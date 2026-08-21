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
impl AcknowledgeTerminalRequest {
    pub fn builder() -> builder::AcknowledgeTerminalRequest {
        Default::default()
    }
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
impl Acknowledgement {
    pub fn builder() -> builder::Acknowledgement {
        Default::default()
    }
}
#[doc = "The single current, transport-neutral Brain to Hand receipt contract. The canonical schema digest is the compatibility identity; the wire carries no protocol version."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"$id\": \"https://github.com/aexhq/brain/contracts/hand/contract.json\","]
#[doc = "  \"title\": \"BrainHandContract\","]
#[doc = "  \"description\": \"The single current, transport-neutral Brain to Hand receipt contract. The canonical schema digest is the compatibility identity; the wire carries no protocol version.\","]
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
pub struct BrainHandContract {
    pub contract: BrainHandContractContract,
}
impl BrainHandContract {
    pub fn builder() -> builder::BrainHandContract {
        Default::default()
    }
}
#[doc = "`BrainHandContractContract`"]
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
pub struct BrainHandContractContract {
    pub methods: ::serde_json::Value,
}
impl BrainHandContractContract {
    pub fn builder() -> builder::BrainHandContractContract {
        Default::default()
    }
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
#[doc = "    \"object\","]
#[doc = "    \"required_env\","]
#[doc = "    \"runtime\","]
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
#[doc = "    \"runtime\": {"]
#[doc = "      \"$ref\": \"#/definitions/BundleRuntime\""]
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
    pub object: ObjectReference,
    pub required_env: Vec<Identifier>,
    pub runtime: BundleRuntime,
    pub tool_name: Identifier,
}
impl BundleDescriptor {
    pub fn builder() -> builder::BundleDescriptor {
        Default::default()
    }
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
impl BundleFetch {
    pub fn builder() -> builder::BundleFetch {
        Default::default()
    }
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
#[doc = "`BundleRuntime`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"node22\""]
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
pub enum BundleRuntime {
    #[serde(rename = "node22")]
    Node22,
}
impl ::std::fmt::Display for BundleRuntime {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Node22 => f.write_str("node22"),
        }
    }
}
impl ::std::str::FromStr for BundleRuntime {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "node22" => Ok(Self::Node22),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for BundleRuntime {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for BundleRuntime {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for BundleRuntime {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
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
impl CancelRequest {
    pub fn builder() -> builder::CancelRequest {
        Default::default()
    }
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
impl CancellationReceipt {
    pub fn builder() -> builder::CancellationReceipt {
        Default::default()
    }
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
impl CreateSandboxRequest {
    pub fn builder() -> builder::CreateSandboxRequest {
        Default::default()
    }
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
#[doc = "`ExecutionRealm`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"aex_managed\","]
#[doc = "    \"customer_app\","]
#[doc = "    \"engine\""]
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
pub enum ExecutionRealm {
    #[serde(rename = "aex_managed")]
    AexManaged,
    #[serde(rename = "customer_app")]
    CustomerApp,
    #[serde(rename = "engine")]
    Engine,
}
impl ::std::fmt::Display for ExecutionRealm {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::AexManaged => f.write_str("aex_managed"),
            Self::CustomerApp => f.write_str("customer_app"),
            Self::Engine => f.write_str("engine"),
        }
    }
}
impl ::std::str::FromStr for ExecutionRealm {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "aex_managed" => Ok(Self::AexManaged),
            "customer_app" => Ok(Self::CustomerApp),
            "engine" => Ok(Self::Engine),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ExecutionRealm {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ExecutionRealm {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ExecutionRealm {
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
#[doc = "`HandCapability`"]
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
pub enum HandCapability {
    #[serde(rename = "execution")]
    Execution,
    #[serde(rename = "session_preparation")]
    SessionPreparation,
    #[serde(rename = "sandbox_files")]
    SandboxFiles,
    #[serde(rename = "sandbox_control")]
    SandboxControl,
}
impl ::std::fmt::Display for HandCapability {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Execution => f.write_str("execution"),
            Self::SessionPreparation => f.write_str("session_preparation"),
            Self::SandboxFiles => f.write_str("sandbox_files"),
            Self::SandboxControl => f.write_str("sandbox_control"),
        }
    }
}
impl ::std::str::FromStr for HandCapability {
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
impl ::std::convert::TryFrom<&str> for HandCapability {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for HandCapability {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for HandCapability {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`HandError`"]
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
#[doc = "      \"$ref\": \"#/definitions/HandErrorCode\""]
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
pub struct HandError {
    pub code: HandErrorCode,
    #[serde(default, skip_serializing_if = "::serde_json::Map::is_empty")]
    pub details: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    pub message: HandErrorMessage,
    pub retryable: bool,
}
impl HandError {
    pub fn builder() -> builder::HandError {
        Default::default()
    }
}
#[doc = "`HandErrorCode`"]
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
pub enum HandErrorCode {
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
impl ::std::fmt::Display for HandErrorCode {
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
impl ::std::str::FromStr for HandErrorCode {
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
impl ::std::convert::TryFrom<&str> for HandErrorCode {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for HandErrorCode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for HandErrorCode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`HandErrorMessage`"]
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
pub struct HandErrorMessage(::std::string::String);
impl ::std::ops::Deref for HandErrorMessage {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<HandErrorMessage> for ::std::string::String {
    fn from(value: HandErrorMessage) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for HandErrorMessage {
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
impl ::std::convert::TryFrom<&str> for HandErrorMessage {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for HandErrorMessage {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for HandErrorMessage {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for HandErrorMessage {
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
impl ObjectReference {
    pub fn builder() -> builder::ObjectReference {
        Default::default()
    }
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
#[doc = "Short-lived, one-purpose transfer capability minted by Brain-owned storage. transfer_id identifies the reservation/capability; object_id is the immutable source or pending destination identity. GET is valid only for import and PUT only for export; Hands never infer an object-store key. Export returns ObjectReference.object_id exactly equal to this sealed object_id."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Short-lived, one-purpose transfer capability minted by Brain-owned storage. transfer_id identifies the reservation/capability; object_id is the immutable source or pending destination identity. GET is valid only for import and PUT only for export; Hands never infer an object-store key. Export returns ObjectReference.object_id exactly equal to this sealed object_id.\","]
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
impl ObjectTransferAuthority {
    pub fn builder() -> builder::ObjectTransferAuthority {
        Default::default()
    }
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
impl ObserveRequest {
    pub fn builder() -> builder::ObserveRequest {
        Default::default()
    }
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
    pub request_digest: Digest,
    pub resources: ResourceCeiling,
    pub root_id: Identifier,
    pub session_id: Identifier,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub target_ref: ::std::option::Option<OperationEnvelopeTargetRef>,
    pub trace: ::std::collections::HashMap<::std::string::String, OperationEnvelopeTraceValue>,
    pub turn_id: Identifier,
}
impl OperationEnvelope {
    pub fn builder() -> builder::OperationEnvelope {
        Default::default()
    }
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
impl OperationInput {
    pub fn builder() -> builder::OperationInput {
        Default::default()
    }
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
impl OperationObservation {
    pub fn builder() -> builder::OperationObservation {
        Default::default()
    }
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
#[doc = "      \"description\": \"Opaque Hand-issued locator for the accepted physical execution. Brain journals it before observe/cancel/ack; it complements the Hand binding/preparation/target registry and never encodes product routing policy.\","]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"request_digest\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    },"]
#[doc = "    \"target\": {"]
#[doc = "      \"description\": \"Exact rooted logical target accepted for this execution. Control and acknowledgement calls carry it back so Hand can reconcile its root-keyed target registry without a reverse index or scan.\","]
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
    #[doc = "Opaque Hand-issued locator for the accepted physical execution. Brain journals it before observe/cancel/ack; it complements the Hand binding/preparation/target registry and never encodes product routing policy."]
    pub receipt_ref: Identifier,
    pub request_digest: Digest,
    #[doc = "Exact rooted logical target accepted for this execution. Control and acknowledgement calls carry it back so Hand can reconcile its root-keyed target registry without a reverse index or scan."]
    pub target: SandboxTarget,
    #[doc = "Opaque physical target locator paired with generation. It never replaces the rooted logical target."]
    pub target_ref: Identifier,
}
impl OperationRef {
    pub fn builder() -> builder::OperationRef {
        Default::default()
    }
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
#[doc = "One bounded output observation emitted by a Hand. Brain treats it as provisional until the terminal receipt is durably journaled."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"One bounded output observation emitted by a Hand. Brain treats it as provisional until the terminal receipt is durably journaled.\","]
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
impl OutputChunk {
    pub fn builder() -> builder::OutputChunk {
        Default::default()
    }
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
impl PrepareSessionRequest {
    pub fn builder() -> builder::PrepareSessionRequest {
        Default::default()
    }
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
impl PreparedBindingBundles {
    pub fn builder() -> builder::PreparedBindingBundles {
        Default::default()
    }
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
impl PreparedSession {
    pub fn builder() -> builder::PreparedSession {
        Default::default()
    }
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
#[doc = "    \"hand_id\","]
#[doc = "    \"limits\","]
#[doc = "    \"realm\","]
#[doc = "    \"recovery\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"binding_ref\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"capabilities\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/definitions/HandCapability\""]
#[doc = "      },"]
#[doc = "      \"uniqueItems\": true"]
#[doc = "    },"]
#[doc = "    \"hand_id\": {"]
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
#[doc = "    \"realm\": {"]
#[doc = "      \"$ref\": \"#/definitions/ExecutionRealm\""]
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
    pub capabilities: Vec<HandCapability>,
    pub hand_id: Identifier,
    pub limits: ResolvedBindingLimits,
    pub realm: ExecutionRealm,
    pub recovery: RecoveryClass,
}
impl ResolvedBinding {
    pub fn builder() -> builder::ResolvedBinding {
        Default::default()
    }
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
impl ResolvedBindingLimits {
    pub fn builder() -> builder::ResolvedBindingLimits {
        Default::default()
    }
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
impl ResourceCeiling {
    pub fn builder() -> builder::ResourceCeiling {
        Default::default()
    }
}
#[doc = "Effect identity is exact across ambiguous transport delivery. Hand retains and replays the byte-equivalent result for the same operation_id and request_digest until the target is purged; a different digest conflicts before effect."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Effect identity is exact across ambiguous transport delivery. Hand retains and replays the byte-equivalent result for the same operation_id and request_digest until the target is purged; a different digest conflicts before effect.\","]
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
impl SandboxCopyRequest {
    pub fn builder() -> builder::SandboxCopyRequest {
        Default::default()
    }
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
impl SandboxCopyResult {
    pub fn builder() -> builder::SandboxCopyResult {
        Default::default()
    }
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
impl SandboxExecInput {
    pub fn builder() -> builder::SandboxExecInput {
        Default::default()
    }
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
impl SandboxExecutionRequest {
    pub fn builder() -> builder::SandboxExecutionRequest {
        Default::default()
    }
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
impl SandboxFileRequest {
    pub fn builder() -> builder::SandboxFileRequest {
        Default::default()
    }
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
#[doc = "Effect identity is exact across ambiguous transport delivery. Hand retains and replays the byte-equivalent result for the same operation_id and request_digest until the target is purged; a different digest conflicts before effect."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Effect identity is exact across ambiguous transport delivery. Hand retains and replays the byte-equivalent result for the same operation_id and request_digest until the target is purged; a different digest conflicts before effect.\","]
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
impl SandboxFileWriteRequest {
    pub fn builder() -> builder::SandboxFileWriteRequest {
        Default::default()
    }
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
impl SandboxFileWriteResult {
    pub fn builder() -> builder::SandboxFileWriteResult {
        Default::default()
    }
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
impl SandboxStatus {
    pub fn builder() -> builder::SandboxStatus {
        Default::default()
    }
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
impl SandboxTarget {
    pub fn builder() -> builder::SandboxTarget {
        Default::default()
    }
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
#[doc = "    \"implementation_identity\","]
#[doc = "    \"policy_digest\","]
#[doc = "    \"realm\","]
#[doc = "    \"realm_id\","]
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
#[doc = "    \"implementation_identity\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    },"]
#[doc = "    \"policy_digest\": {"]
#[doc = "      \"$ref\": \"#/definitions/Digest\""]
#[doc = "    },"]
#[doc = "    \"realm\": {"]
#[doc = "      \"$ref\": \"#/definitions/ExecutionRealm\""]
#[doc = "    },"]
#[doc = "    \"realm_id\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"required_capabilities\": {"]
#[doc = "      \"default\": ["]
#[doc = "        \"execution\""]
#[doc = "      ],"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/definitions/HandCapability\""]
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
    pub implementation_identity: Digest,
    pub policy_digest: Digest,
    pub realm: ExecutionRealm,
    pub realm_id: Identifier,
    #[serde(default = "defaults::sealed_binding_required_capabilities")]
    pub required_capabilities: Vec<HandCapability>,
    pub root_id: Identifier,
    pub session_id: Identifier,
}
impl SealedBinding {
    pub fn builder() -> builder::SealedBinding {
        Default::default()
    }
}
#[doc = "Opaque, short-lived, one-redemption authority for one session and one physical target generation. The Hand may keep redeemed values only in supervisor memory and inject each binding's declared subset at child spawn. Brain may mint a replacement capability for the same surviving generation after a Hand control-process crash. Secret values never enter this contract, binding registry, journal, receipt or argv."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Opaque, short-lived, one-redemption authority for one session and one physical target generation. The Hand may keep redeemed values only in supervisor memory and inject each binding's declared subset at child spawn. Brain may mint a replacement capability for the same surviving generation after a Hand control-process crash. Secret values never enter this contract, binding registry, journal, receipt or argv.\","]
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
impl SecretCapability {
    pub fn builder() -> builder::SecretCapability {
        Default::default()
    }
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
#[doc = "    \"generation_intent\","]
#[doc = "    \"hand_id\","]
#[doc = "    \"root_id\","]
#[doc = "    \"session_id\","]
#[doc = "    \"target\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"capability_ref\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"generation_intent\": {"]
#[doc = "      \"$ref\": \"#/definitions/Identifier\""]
#[doc = "    },"]
#[doc = "    \"hand_id\": {"]
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
    pub generation_intent: Identifier,
    pub hand_id: Identifier,
    pub root_id: Identifier,
    pub session_id: Identifier,
    pub target: SandboxTarget,
}
impl SecretDeliveryRequest {
    pub fn builder() -> builder::SecretDeliveryRequest {
        Default::default()
    }
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
impl SubmitReceipt {
    pub fn builder() -> builder::SubmitReceipt {
        Default::default()
    }
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
impl SubmitRequest {
    pub fn builder() -> builder::SubmitRequest {
        Default::default()
    }
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
#[doc = "Hand-issued continuity locator for a materialized target. Brain journals and projects the newest receipt, then supplies target_ref and generation on later operations."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Hand-issued continuity locator for a materialized target. Brain journals and projects the newest receipt, then supplies target_ref and generation on later operations.\","]
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
impl TargetReceipt {
    pub fn builder() -> builder::TargetReceipt {
        Default::default()
    }
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
impl TerminalResult {
    pub fn builder() -> builder::TerminalResult {
        Default::default()
    }
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
impl WriteStdinReceipt {
    pub fn builder() -> builder::WriteStdinReceipt {
        Default::default()
    }
}
#[doc = "One idempotent stdin append/EOF/poll. Empty text with eof=false is a pure poll. UTF-8 payload bytes are additionally capped at 4096 so the Hand can perform one PIPE_BUF-bounded write; larger input must be split into separately identified requests."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"One idempotent stdin append/EOF/poll. Empty text with eof=false is a pure poll. UTF-8 payload bytes are additionally capped at 4096 so the Hand can perform one PIPE_BUF-bounded write; larger input must be split into separately identified requests.\","]
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
impl WriteStdinRequest {
    pub fn builder() -> builder::WriteStdinRequest {
        Default::default()
    }
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
#[doc = r" Types for composing complex structures."]
pub mod builder {
    #[derive(Clone, Debug)]
    pub struct AcknowledgeTerminalRequest {
        operation: ::std::result::Result<super::OperationRef, ::std::string::String>,
        terminal_digest: ::std::result::Result<super::Digest, ::std::string::String>,
    }
    impl ::std::default::Default for AcknowledgeTerminalRequest {
        fn default() -> Self {
            Self {
                operation: Err("no value supplied for operation".to_string()),
                terminal_digest: Err("no value supplied for terminal_digest".to_string()),
            }
        }
    }
    impl AcknowledgeTerminalRequest {
        pub fn operation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationRef>,
            T::Error: ::std::fmt::Display,
        {
            self.operation = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for operation: {e}"));
            self
        }
        pub fn terminal_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Digest>,
            T::Error: ::std::fmt::Display,
        {
            self.terminal_digest = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for terminal_digest: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<AcknowledgeTerminalRequest> for super::AcknowledgeTerminalRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: AcknowledgeTerminalRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                operation: value.operation?,
                terminal_digest: value.terminal_digest?,
            })
        }
    }
    impl ::std::convert::From<super::AcknowledgeTerminalRequest> for AcknowledgeTerminalRequest {
        fn from(value: super::AcknowledgeTerminalRequest) -> Self {
            Self {
                operation: Ok(value.operation),
                terminal_digest: Ok(value.terminal_digest),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct Acknowledgement {
        acknowledged: ::std::result::Result<bool, ::std::string::String>,
    }
    impl ::std::default::Default for Acknowledgement {
        fn default() -> Self {
            Self {
                acknowledged: Err("no value supplied for acknowledged".to_string()),
            }
        }
    }
    impl Acknowledgement {
        pub fn acknowledged<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<bool>,
            T::Error: ::std::fmt::Display,
        {
            self.acknowledged = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for acknowledged: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<Acknowledgement> for super::Acknowledgement {
        type Error = super::error::ConversionError;
        fn try_from(
            value: Acknowledgement,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                acknowledged: value.acknowledged?,
            })
        }
    }
    impl ::std::convert::From<super::Acknowledgement> for Acknowledgement {
        fn from(value: super::Acknowledgement) -> Self {
            Self {
                acknowledged: Ok(value.acknowledged),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct BrainHandContract {
        contract: ::std::result::Result<super::BrainHandContractContract, ::std::string::String>,
    }
    impl ::std::default::Default for BrainHandContract {
        fn default() -> Self {
            Self {
                contract: Err("no value supplied for contract".to_string()),
            }
        }
    }
    impl BrainHandContract {
        pub fn contract<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::BrainHandContractContract>,
            T::Error: ::std::fmt::Display,
        {
            self.contract = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for contract: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<BrainHandContract> for super::BrainHandContract {
        type Error = super::error::ConversionError;
        fn try_from(
            value: BrainHandContract,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                contract: value.contract?,
            })
        }
    }
    impl ::std::convert::From<super::BrainHandContract> for BrainHandContract {
        fn from(value: super::BrainHandContract) -> Self {
            Self {
                contract: Ok(value.contract),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct BrainHandContractContract {
        methods: ::std::result::Result<::serde_json::Value, ::std::string::String>,
    }
    impl ::std::default::Default for BrainHandContractContract {
        fn default() -> Self {
            Self {
                methods: Err("no value supplied for methods".to_string()),
            }
        }
    }
    impl BrainHandContractContract {
        pub fn methods<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::serde_json::Value>,
            T::Error: ::std::fmt::Display,
        {
            self.methods = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for methods: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<BrainHandContractContract> for super::BrainHandContractContract {
        type Error = super::error::ConversionError;
        fn try_from(
            value: BrainHandContractContract,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                methods: value.methods?,
            })
        }
    }
    impl ::std::convert::From<super::BrainHandContractContract> for BrainHandContractContract {
        fn from(value: super::BrainHandContractContract) -> Self {
            Self {
                methods: Ok(value.methods),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct BundleDescriptor {
        bundle_digest: ::std::result::Result<super::Digest, ::std::string::String>,
        bytes: ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
        contract_digest: ::std::result::Result<super::Digest, ::std::string::String>,
        description: ::std::result::Result<
            ::std::option::Option<super::BundleDescriptorDescription>,
            ::std::string::String,
        >,
        object: ::std::result::Result<super::ObjectReference, ::std::string::String>,
        required_env: ::std::result::Result<Vec<super::Identifier>, ::std::string::String>,
        runtime: ::std::result::Result<super::BundleRuntime, ::std::string::String>,
        tool_name: ::std::result::Result<super::Identifier, ::std::string::String>,
    }
    impl ::std::default::Default for BundleDescriptor {
        fn default() -> Self {
            Self {
                bundle_digest: Err("no value supplied for bundle_digest".to_string()),
                bytes: Err("no value supplied for bytes".to_string()),
                contract_digest: Err("no value supplied for contract_digest".to_string()),
                description: Ok(Default::default()),
                object: Err("no value supplied for object".to_string()),
                required_env: Err("no value supplied for required_env".to_string()),
                runtime: Err("no value supplied for runtime".to_string()),
                tool_name: Err("no value supplied for tool_name".to_string()),
            }
        }
    }
    impl BundleDescriptor {
        pub fn bundle_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Digest>,
            T::Error: ::std::fmt::Display,
        {
            self.bundle_digest = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for bundle_digest: {e}"));
            self
        }
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
        pub fn contract_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Digest>,
            T::Error: ::std::fmt::Display,
        {
            self.contract_digest = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for contract_digest: {e}"));
            self
        }
        pub fn description<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::BundleDescriptorDescription>>,
            T::Error: ::std::fmt::Display,
        {
            self.description = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for description: {e}"));
            self
        }
        pub fn object<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ObjectReference>,
            T::Error: ::std::fmt::Display,
        {
            self.object = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for object: {e}"));
            self
        }
        pub fn required_env<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<Vec<super::Identifier>>,
            T::Error: ::std::fmt::Display,
        {
            self.required_env = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for required_env: {e}"));
            self
        }
        pub fn runtime<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::BundleRuntime>,
            T::Error: ::std::fmt::Display,
        {
            self.runtime = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for runtime: {e}"));
            self
        }
        pub fn tool_name<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.tool_name = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for tool_name: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<BundleDescriptor> for super::BundleDescriptor {
        type Error = super::error::ConversionError;
        fn try_from(
            value: BundleDescriptor,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                bundle_digest: value.bundle_digest?,
                bytes: value.bytes?,
                contract_digest: value.contract_digest?,
                description: value.description?,
                object: value.object?,
                required_env: value.required_env?,
                runtime: value.runtime?,
                tool_name: value.tool_name?,
            })
        }
    }
    impl ::std::convert::From<super::BundleDescriptor> for BundleDescriptor {
        fn from(value: super::BundleDescriptor) -> Self {
            Self {
                bundle_digest: Ok(value.bundle_digest),
                bytes: Ok(value.bytes),
                contract_digest: Ok(value.contract_digest),
                description: Ok(value.description),
                object: Ok(value.object),
                required_env: Ok(value.required_env),
                runtime: Ok(value.runtime),
                tool_name: Ok(value.tool_name),
            }
        }
    }
    #[derive(Clone)]
    pub struct BundleFetch {
        bundle_digest: ::std::result::Result<super::Digest, ::std::string::String>,
        expires_at_ms: ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
        headers: ::std::result::Result<
            ::std::collections::HashMap<::std::string::String, super::BundleFetchHeadersValue>,
            ::std::string::String,
        >,
        max_bytes: ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
        url: ::std::result::Result<super::BundleFetchUrl, ::std::string::String>,
    }
    impl ::std::default::Default for BundleFetch {
        fn default() -> Self {
            Self {
                bundle_digest: Err("no value supplied for bundle_digest".to_string()),
                expires_at_ms: Err("no value supplied for expires_at_ms".to_string()),
                headers: Err("no value supplied for headers".to_string()),
                max_bytes: Err("no value supplied for max_bytes".to_string()),
                url: Err("no value supplied for url".to_string()),
            }
        }
    }
    impl BundleFetch {
        pub fn bundle_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Digest>,
            T::Error: ::std::fmt::Display,
        {
            self.bundle_digest = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for bundle_digest: {e}"));
            self
        }
        pub fn expires_at_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::num::NonZeroU64>,
            T::Error: ::std::fmt::Display,
        {
            self.expires_at_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for expires_at_ms: {e}"));
            self
        }
        pub fn headers<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::std::collections::HashMap<
                        ::std::string::String,
                        super::BundleFetchHeadersValue,
                    >,
                >,
            T::Error: ::std::fmt::Display,
        {
            self.headers = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for headers: {e}"));
            self
        }
        pub fn max_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::num::NonZeroU64>,
            T::Error: ::std::fmt::Display,
        {
            self.max_bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for max_bytes: {e}"));
            self
        }
        pub fn url<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::BundleFetchUrl>,
            T::Error: ::std::fmt::Display,
        {
            self.url = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for url: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<BundleFetch> for super::BundleFetch {
        type Error = super::error::ConversionError;
        fn try_from(
            value: BundleFetch,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                bundle_digest: value.bundle_digest?,
                expires_at_ms: value.expires_at_ms?,
                headers: value.headers?,
                max_bytes: value.max_bytes?,
                url: value.url?,
            })
        }
    }
    impl ::std::convert::From<super::BundleFetch> for BundleFetch {
        fn from(value: super::BundleFetch) -> Self {
            Self {
                bundle_digest: Ok(value.bundle_digest),
                expires_at_ms: Ok(value.expires_at_ms),
                headers: Ok(value.headers),
                max_bytes: Ok(value.max_bytes),
                url: Ok(value.url),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct CancelRequest {
        operation: ::std::result::Result<super::OperationRef, ::std::string::String>,
        reason: ::std::result::Result<super::CancelRequestReason, ::std::string::String>,
    }
    impl ::std::default::Default for CancelRequest {
        fn default() -> Self {
            Self {
                operation: Err("no value supplied for operation".to_string()),
                reason: Err("no value supplied for reason".to_string()),
            }
        }
    }
    impl CancelRequest {
        pub fn operation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationRef>,
            T::Error: ::std::fmt::Display,
        {
            self.operation = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for operation: {e}"));
            self
        }
        pub fn reason<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::CancelRequestReason>,
            T::Error: ::std::fmt::Display,
        {
            self.reason = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for reason: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<CancelRequest> for super::CancelRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: CancelRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                operation: value.operation?,
                reason: value.reason?,
            })
        }
    }
    impl ::std::convert::From<super::CancelRequest> for CancelRequest {
        fn from(value: super::CancelRequest) -> Self {
            Self {
                operation: Ok(value.operation),
                reason: Ok(value.reason),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct CancellationReceipt {
        accepted: ::std::result::Result<bool, ::std::string::String>,
        observation: ::std::result::Result<super::OperationObservation, ::std::string::String>,
        operation: ::std::result::Result<super::OperationRef, ::std::string::String>,
    }
    impl ::std::default::Default for CancellationReceipt {
        fn default() -> Self {
            Self {
                accepted: Err("no value supplied for accepted".to_string()),
                observation: Err("no value supplied for observation".to_string()),
                operation: Err("no value supplied for operation".to_string()),
            }
        }
    }
    impl CancellationReceipt {
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
        pub fn observation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationObservation>,
            T::Error: ::std::fmt::Display,
        {
            self.observation = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for observation: {e}"));
            self
        }
        pub fn operation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationRef>,
            T::Error: ::std::fmt::Display,
        {
            self.operation = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for operation: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<CancellationReceipt> for super::CancellationReceipt {
        type Error = super::error::ConversionError;
        fn try_from(
            value: CancellationReceipt,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                accepted: value.accepted?,
                observation: value.observation?,
                operation: value.operation?,
            })
        }
    }
    impl ::std::convert::From<super::CancellationReceipt> for CancellationReceipt {
        fn from(value: super::CancellationReceipt) -> Self {
            Self {
                accepted: Ok(value.accepted),
                observation: Ok(value.observation),
                operation: Ok(value.operation),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct CreateSandboxRequest {
        generation_intent: ::std::result::Result<super::Identifier, ::std::string::String>,
        network: ::std::result::Result<super::NetworkCeiling, ::std::string::String>,
        resource_class: ::std::result::Result<super::Identifier, ::std::string::String>,
        resources: ::std::result::Result<super::ResourceCeiling, ::std::string::String>,
        target: ::std::result::Result<super::SandboxTarget, ::std::string::String>,
    }
    impl ::std::default::Default for CreateSandboxRequest {
        fn default() -> Self {
            Self {
                generation_intent: Err("no value supplied for generation_intent".to_string()),
                network: Err("no value supplied for network".to_string()),
                resource_class: Err("no value supplied for resource_class".to_string()),
                resources: Err("no value supplied for resources".to_string()),
                target: Err("no value supplied for target".to_string()),
            }
        }
    }
    impl CreateSandboxRequest {
        pub fn generation_intent<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.generation_intent = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for generation_intent: {e}"));
            self
        }
        pub fn network<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::NetworkCeiling>,
            T::Error: ::std::fmt::Display,
        {
            self.network = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for network: {e}"));
            self
        }
        pub fn resource_class<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.resource_class = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for resource_class: {e}"));
            self
        }
        pub fn resources<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ResourceCeiling>,
            T::Error: ::std::fmt::Display,
        {
            self.resources = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for resources: {e}"));
            self
        }
        pub fn target<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SandboxTarget>,
            T::Error: ::std::fmt::Display,
        {
            self.target = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for target: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<CreateSandboxRequest> for super::CreateSandboxRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: CreateSandboxRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                generation_intent: value.generation_intent?,
                network: value.network?,
                resource_class: value.resource_class?,
                resources: value.resources?,
                target: value.target?,
            })
        }
    }
    impl ::std::convert::From<super::CreateSandboxRequest> for CreateSandboxRequest {
        fn from(value: super::CreateSandboxRequest) -> Self {
            Self {
                generation_intent: Ok(value.generation_intent),
                network: Ok(value.network),
                resource_class: Ok(value.resource_class),
                resources: Ok(value.resources),
                target: Ok(value.target),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct FileEntry {
        bytes: ::std::result::Result<u64, ::std::string::String>,
        kind: ::std::result::Result<super::FileEntryKind, ::std::string::String>,
        modified_at_ms: ::std::result::Result<u64, ::std::string::String>,
        path: ::std::result::Result<super::FileEntryPath, ::std::string::String>,
        sha256: ::std::result::Result<::std::option::Option<super::Digest>, ::std::string::String>,
    }
    impl ::std::default::Default for FileEntry {
        fn default() -> Self {
            Self {
                bytes: Err("no value supplied for bytes".to_string()),
                kind: Err("no value supplied for kind".to_string()),
                modified_at_ms: Err("no value supplied for modified_at_ms".to_string()),
                path: Err("no value supplied for path".to_string()),
                sha256: Ok(Default::default()),
            }
        }
    }
    impl FileEntry {
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
        pub fn modified_at_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.modified_at_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for modified_at_ms: {e}"));
            self
        }
        pub fn path<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::FileEntryPath>,
            T::Error: ::std::fmt::Display,
        {
            self.path = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for path: {e}"));
            self
        }
        pub fn sha256<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::Digest>>,
            T::Error: ::std::fmt::Display,
        {
            self.sha256 = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for sha256: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<FileEntry> for super::FileEntry {
        type Error = super::error::ConversionError;
        fn try_from(
            value: FileEntry,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                bytes: value.bytes?,
                kind: value.kind?,
                modified_at_ms: value.modified_at_ms?,
                path: value.path?,
                sha256: value.sha256?,
            })
        }
    }
    impl ::std::convert::From<super::FileEntry> for FileEntry {
        fn from(value: super::FileEntry) -> Self {
            Self {
                bytes: Ok(value.bytes),
                kind: Ok(value.kind),
                modified_at_ms: Ok(value.modified_at_ms),
                path: Ok(value.path),
                sha256: Ok(value.sha256),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct HandError {
        code: ::std::result::Result<super::HandErrorCode, ::std::string::String>,
        details: ::std::result::Result<
            ::serde_json::Map<::std::string::String, ::serde_json::Value>,
            ::std::string::String,
        >,
        message: ::std::result::Result<super::HandErrorMessage, ::std::string::String>,
        retryable: ::std::result::Result<bool, ::std::string::String>,
    }
    impl ::std::default::Default for HandError {
        fn default() -> Self {
            Self {
                code: Err("no value supplied for code".to_string()),
                details: Ok(Default::default()),
                message: Err("no value supplied for message".to_string()),
                retryable: Err("no value supplied for retryable".to_string()),
            }
        }
    }
    impl HandError {
        pub fn code<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::HandErrorCode>,
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
            T: ::std::convert::TryInto<super::HandErrorMessage>,
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
    impl ::std::convert::TryFrom<HandError> for super::HandError {
        type Error = super::error::ConversionError;
        fn try_from(
            value: HandError,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                code: value.code?,
                details: value.details?,
                message: value.message?,
                retryable: value.retryable?,
            })
        }
    }
    impl ::std::convert::From<super::HandError> for HandError {
        fn from(value: super::HandError) -> Self {
            Self {
                code: Ok(value.code),
                details: Ok(value.details),
                message: Ok(value.message),
                retryable: Ok(value.retryable),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ObjectReference {
        bytes: ::std::result::Result<u64, ::std::string::String>,
        media_type: ::std::result::Result<
            ::std::option::Option<super::ObjectReferenceMediaType>,
            ::std::string::String,
        >,
        object_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        sha256: ::std::result::Result<super::Digest, ::std::string::String>,
    }
    impl ::std::default::Default for ObjectReference {
        fn default() -> Self {
            Self {
                bytes: Err("no value supplied for bytes".to_string()),
                media_type: Ok(Default::default()),
                object_id: Err("no value supplied for object_id".to_string()),
                sha256: Err("no value supplied for sha256".to_string()),
            }
        }
    }
    impl ObjectReference {
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
            T: ::std::convert::TryInto<::std::option::Option<super::ObjectReferenceMediaType>>,
            T::Error: ::std::fmt::Display,
        {
            self.media_type = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for media_type: {e}"));
            self
        }
        pub fn object_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.object_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for object_id: {e}"));
            self
        }
        pub fn sha256<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Digest>,
            T::Error: ::std::fmt::Display,
        {
            self.sha256 = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for sha256: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ObjectReference> for super::ObjectReference {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ObjectReference,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                bytes: value.bytes?,
                media_type: value.media_type?,
                object_id: value.object_id?,
                sha256: value.sha256?,
            })
        }
    }
    impl ::std::convert::From<super::ObjectReference> for ObjectReference {
        fn from(value: super::ObjectReference) -> Self {
            Self {
                bytes: Ok(value.bytes),
                media_type: Ok(value.media_type),
                object_id: Ok(value.object_id),
                sha256: Ok(value.sha256),
            }
        }
    }
    #[derive(Clone)]
    pub struct ObjectTransferAuthority {
        expires_at_ms: ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
        headers: ::std::result::Result<
            ::std::collections::HashMap<
                ::std::string::String,
                super::ObjectTransferAuthorityHeadersValue,
            >,
            ::std::string::String,
        >,
        max_bytes: ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
        method: ::std::result::Result<super::ObjectTransferAuthorityMethod, ::std::string::String>,
        object_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        transfer_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        url: ::std::result::Result<super::ObjectTransferAuthorityUrl, ::std::string::String>,
    }
    impl ::std::default::Default for ObjectTransferAuthority {
        fn default() -> Self {
            Self {
                expires_at_ms: Err("no value supplied for expires_at_ms".to_string()),
                headers: Err("no value supplied for headers".to_string()),
                max_bytes: Err("no value supplied for max_bytes".to_string()),
                method: Err("no value supplied for method".to_string()),
                object_id: Err("no value supplied for object_id".to_string()),
                transfer_id: Err("no value supplied for transfer_id".to_string()),
                url: Err("no value supplied for url".to_string()),
            }
        }
    }
    impl ObjectTransferAuthority {
        pub fn expires_at_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::num::NonZeroU64>,
            T::Error: ::std::fmt::Display,
        {
            self.expires_at_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for expires_at_ms: {e}"));
            self
        }
        pub fn headers<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::std::collections::HashMap<
                        ::std::string::String,
                        super::ObjectTransferAuthorityHeadersValue,
                    >,
                >,
            T::Error: ::std::fmt::Display,
        {
            self.headers = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for headers: {e}"));
            self
        }
        pub fn max_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::num::NonZeroU64>,
            T::Error: ::std::fmt::Display,
        {
            self.max_bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for max_bytes: {e}"));
            self
        }
        pub fn method<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ObjectTransferAuthorityMethod>,
            T::Error: ::std::fmt::Display,
        {
            self.method = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for method: {e}"));
            self
        }
        pub fn object_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.object_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for object_id: {e}"));
            self
        }
        pub fn transfer_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.transfer_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for transfer_id: {e}"));
            self
        }
        pub fn url<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ObjectTransferAuthorityUrl>,
            T::Error: ::std::fmt::Display,
        {
            self.url = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for url: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ObjectTransferAuthority> for super::ObjectTransferAuthority {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ObjectTransferAuthority,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                expires_at_ms: value.expires_at_ms?,
                headers: value.headers?,
                max_bytes: value.max_bytes?,
                method: value.method?,
                object_id: value.object_id?,
                transfer_id: value.transfer_id?,
                url: value.url?,
            })
        }
    }
    impl ::std::convert::From<super::ObjectTransferAuthority> for ObjectTransferAuthority {
        fn from(value: super::ObjectTransferAuthority) -> Self {
            Self {
                expires_at_ms: Ok(value.expires_at_ms),
                headers: Ok(value.headers),
                max_bytes: Ok(value.max_bytes),
                method: Ok(value.method),
                object_id: Ok(value.object_id),
                transfer_id: Ok(value.transfer_id),
                url: Ok(value.url),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ObserveRequest {
        cursor: ::std::result::Result<super::ObserveRequestCursor, ::std::string::String>,
        operation: ::std::result::Result<super::OperationRef, ::std::string::String>,
        wait_ms: ::std::result::Result<u64, ::std::string::String>,
    }
    impl ::std::default::Default for ObserveRequest {
        fn default() -> Self {
            Self {
                cursor: Err("no value supplied for cursor".to_string()),
                operation: Err("no value supplied for operation".to_string()),
                wait_ms: Err("no value supplied for wait_ms".to_string()),
            }
        }
    }
    impl ObserveRequest {
        pub fn cursor<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ObserveRequestCursor>,
            T::Error: ::std::fmt::Display,
        {
            self.cursor = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for cursor: {e}"));
            self
        }
        pub fn operation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationRef>,
            T::Error: ::std::fmt::Display,
        {
            self.operation = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for operation: {e}"));
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
    impl ::std::convert::TryFrom<ObserveRequest> for super::ObserveRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ObserveRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                cursor: value.cursor?,
                operation: value.operation?,
                wait_ms: value.wait_ms?,
            })
        }
    }
    impl ::std::convert::From<super::ObserveRequest> for ObserveRequest {
        fn from(value: super::ObserveRequest) -> Self {
            Self {
                cursor: Ok(value.cursor),
                operation: Ok(value.operation),
                wait_ms: Ok(value.wait_ms),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct OperationEnvelope {
        binding_ref: ::std::result::Result<super::Identifier, ::std::string::String>,
        caller_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        capability: ::std::result::Result<super::Identifier, ::std::string::String>,
        deadline_at_ms: ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
        fence: ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
        generation: ::std::result::Result<
            ::std::option::Option<super::OperationEnvelopeGeneration>,
            ::std::string::String,
        >,
        input: ::std::result::Result<super::OperationInput, ::std::string::String>,
        network: ::std::result::Result<super::NetworkCeiling, ::std::string::String>,
        operation_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        request_digest: ::std::result::Result<super::Digest, ::std::string::String>,
        resources: ::std::result::Result<super::ResourceCeiling, ::std::string::String>,
        root_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        session_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        target_ref: ::std::result::Result<
            ::std::option::Option<super::OperationEnvelopeTargetRef>,
            ::std::string::String,
        >,
        trace: ::std::result::Result<
            ::std::collections::HashMap<::std::string::String, super::OperationEnvelopeTraceValue>,
            ::std::string::String,
        >,
        turn_id: ::std::result::Result<super::Identifier, ::std::string::String>,
    }
    impl ::std::default::Default for OperationEnvelope {
        fn default() -> Self {
            Self {
                binding_ref: Err("no value supplied for binding_ref".to_string()),
                caller_id: Err("no value supplied for caller_id".to_string()),
                capability: Err("no value supplied for capability".to_string()),
                deadline_at_ms: Err("no value supplied for deadline_at_ms".to_string()),
                fence: Err("no value supplied for fence".to_string()),
                generation: Ok(Default::default()),
                input: Err("no value supplied for input".to_string()),
                network: Err("no value supplied for network".to_string()),
                operation_id: Err("no value supplied for operation_id".to_string()),
                request_digest: Err("no value supplied for request_digest".to_string()),
                resources: Err("no value supplied for resources".to_string()),
                root_id: Err("no value supplied for root_id".to_string()),
                session_id: Err("no value supplied for session_id".to_string()),
                target_ref: Ok(Default::default()),
                trace: Err("no value supplied for trace".to_string()),
                turn_id: Err("no value supplied for turn_id".to_string()),
            }
        }
    }
    impl OperationEnvelope {
        pub fn binding_ref<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.binding_ref = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for binding_ref: {e}"));
            self
        }
        pub fn caller_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.caller_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for caller_id: {e}"));
            self
        }
        pub fn capability<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.capability = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for capability: {e}"));
            self
        }
        pub fn deadline_at_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::num::NonZeroU64>,
            T::Error: ::std::fmt::Display,
        {
            self.deadline_at_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for deadline_at_ms: {e}"));
            self
        }
        pub fn fence<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::num::NonZeroU64>,
            T::Error: ::std::fmt::Display,
        {
            self.fence = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for fence: {e}"));
            self
        }
        pub fn generation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::OperationEnvelopeGeneration>>,
            T::Error: ::std::fmt::Display,
        {
            self.generation = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for generation: {e}"));
            self
        }
        pub fn input<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationInput>,
            T::Error: ::std::fmt::Display,
        {
            self.input = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for input: {e}"));
            self
        }
        pub fn network<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::NetworkCeiling>,
            T::Error: ::std::fmt::Display,
        {
            self.network = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for network: {e}"));
            self
        }
        pub fn operation_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.operation_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for operation_id: {e}"));
            self
        }
        pub fn request_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Digest>,
            T::Error: ::std::fmt::Display,
        {
            self.request_digest = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for request_digest: {e}"));
            self
        }
        pub fn resources<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ResourceCeiling>,
            T::Error: ::std::fmt::Display,
        {
            self.resources = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for resources: {e}"));
            self
        }
        pub fn root_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.root_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for root_id: {e}"));
            self
        }
        pub fn session_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.session_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for session_id: {e}"));
            self
        }
        pub fn target_ref<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::OperationEnvelopeTargetRef>>,
            T::Error: ::std::fmt::Display,
        {
            self.target_ref = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for target_ref: {e}"));
            self
        }
        pub fn trace<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<
                    ::std::collections::HashMap<
                        ::std::string::String,
                        super::OperationEnvelopeTraceValue,
                    >,
                >,
            T::Error: ::std::fmt::Display,
        {
            self.trace = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for trace: {e}"));
            self
        }
        pub fn turn_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.turn_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for turn_id: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<OperationEnvelope> for super::OperationEnvelope {
        type Error = super::error::ConversionError;
        fn try_from(
            value: OperationEnvelope,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                binding_ref: value.binding_ref?,
                caller_id: value.caller_id?,
                capability: value.capability?,
                deadline_at_ms: value.deadline_at_ms?,
                fence: value.fence?,
                generation: value.generation?,
                input: value.input?,
                network: value.network?,
                operation_id: value.operation_id?,
                request_digest: value.request_digest?,
                resources: value.resources?,
                root_id: value.root_id?,
                session_id: value.session_id?,
                target_ref: value.target_ref?,
                trace: value.trace?,
                turn_id: value.turn_id?,
            })
        }
    }
    impl ::std::convert::From<super::OperationEnvelope> for OperationEnvelope {
        fn from(value: super::OperationEnvelope) -> Self {
            Self {
                binding_ref: Ok(value.binding_ref),
                caller_id: Ok(value.caller_id),
                capability: Ok(value.capability),
                deadline_at_ms: Ok(value.deadline_at_ms),
                fence: Ok(value.fence),
                generation: Ok(value.generation),
                input: Ok(value.input),
                network: Ok(value.network),
                operation_id: Ok(value.operation_id),
                request_digest: Ok(value.request_digest),
                resources: Ok(value.resources),
                root_id: Ok(value.root_id),
                session_id: Ok(value.session_id),
                target_ref: Ok(value.target_ref),
                trace: Ok(value.trace),
                turn_id: Ok(value.turn_id),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct OperationInput {
        kind: ::std::result::Result<::serde_json::Value, ::std::string::String>,
        value: ::std::result::Result<::serde_json::Value, ::std::string::String>,
    }
    impl ::std::default::Default for OperationInput {
        fn default() -> Self {
            Self {
                kind: Err("no value supplied for kind".to_string()),
                value: Err("no value supplied for value".to_string()),
            }
        }
    }
    impl OperationInput {
        pub fn kind<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::serde_json::Value>,
            T::Error: ::std::fmt::Display,
        {
            self.kind = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for kind: {e}"));
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
    impl ::std::convert::TryFrom<OperationInput> for super::OperationInput {
        type Error = super::error::ConversionError;
        fn try_from(
            value: OperationInput,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                kind: value.kind?,
                value: value.value?,
            })
        }
    }
    impl ::std::convert::From<super::OperationInput> for OperationInput {
        fn from(value: super::OperationInput) -> Self {
            Self {
                kind: Ok(value.kind),
                value: Ok(value.value),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct OperationObservation {
        next_cursor:
            ::std::result::Result<super::OperationObservationNextCursor, ::std::string::String>,
        operation: ::std::result::Result<super::OperationRef, ::std::string::String>,
        output: ::std::result::Result<::std::vec::Vec<super::OutputChunk>, ::std::string::String>,
        state: ::std::result::Result<super::OperationState, ::std::string::String>,
        target: ::std::result::Result<
            ::std::option::Option<super::TargetReceipt>,
            ::std::string::String,
        >,
        terminal: ::std::result::Result<
            ::std::option::Option<super::TerminalResult>,
            ::std::string::String,
        >,
    }
    impl ::std::default::Default for OperationObservation {
        fn default() -> Self {
            Self {
                next_cursor: Err("no value supplied for next_cursor".to_string()),
                operation: Err("no value supplied for operation".to_string()),
                output: Err("no value supplied for output".to_string()),
                state: Err("no value supplied for state".to_string()),
                target: Ok(Default::default()),
                terminal: Ok(Default::default()),
            }
        }
    }
    impl OperationObservation {
        pub fn next_cursor<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationObservationNextCursor>,
            T::Error: ::std::fmt::Display,
        {
            self.next_cursor = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for next_cursor: {e}"));
            self
        }
        pub fn operation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationRef>,
            T::Error: ::std::fmt::Display,
        {
            self.operation = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for operation: {e}"));
            self
        }
        pub fn output<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::vec::Vec<super::OutputChunk>>,
            T::Error: ::std::fmt::Display,
        {
            self.output = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for output: {e}"));
            self
        }
        pub fn state<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationState>,
            T::Error: ::std::fmt::Display,
        {
            self.state = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for state: {e}"));
            self
        }
        pub fn target<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::TargetReceipt>>,
            T::Error: ::std::fmt::Display,
        {
            self.target = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for target: {e}"));
            self
        }
        pub fn terminal<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::TerminalResult>>,
            T::Error: ::std::fmt::Display,
        {
            self.terminal = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for terminal: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<OperationObservation> for super::OperationObservation {
        type Error = super::error::ConversionError;
        fn try_from(
            value: OperationObservation,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                next_cursor: value.next_cursor?,
                operation: value.operation?,
                output: value.output?,
                state: value.state?,
                target: value.target?,
                terminal: value.terminal?,
            })
        }
    }
    impl ::std::convert::From<super::OperationObservation> for OperationObservation {
        fn from(value: super::OperationObservation) -> Self {
            Self {
                next_cursor: Ok(value.next_cursor),
                operation: Ok(value.operation),
                output: Ok(value.output),
                state: Ok(value.state),
                target: Ok(value.target),
                terminal: Ok(value.terminal),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct OperationRef {
        generation: ::std::result::Result<super::Identifier, ::std::string::String>,
        operation_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        receipt_ref: ::std::result::Result<super::Identifier, ::std::string::String>,
        request_digest: ::std::result::Result<super::Digest, ::std::string::String>,
        target: ::std::result::Result<super::SandboxTarget, ::std::string::String>,
        target_ref: ::std::result::Result<super::Identifier, ::std::string::String>,
    }
    impl ::std::default::Default for OperationRef {
        fn default() -> Self {
            Self {
                generation: Err("no value supplied for generation".to_string()),
                operation_id: Err("no value supplied for operation_id".to_string()),
                receipt_ref: Err("no value supplied for receipt_ref".to_string()),
                request_digest: Err("no value supplied for request_digest".to_string()),
                target: Err("no value supplied for target".to_string()),
                target_ref: Err("no value supplied for target_ref".to_string()),
            }
        }
    }
    impl OperationRef {
        pub fn generation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.generation = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for generation: {e}"));
            self
        }
        pub fn operation_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.operation_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for operation_id: {e}"));
            self
        }
        pub fn receipt_ref<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.receipt_ref = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for receipt_ref: {e}"));
            self
        }
        pub fn request_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Digest>,
            T::Error: ::std::fmt::Display,
        {
            self.request_digest = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for request_digest: {e}"));
            self
        }
        pub fn target<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SandboxTarget>,
            T::Error: ::std::fmt::Display,
        {
            self.target = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for target: {e}"));
            self
        }
        pub fn target_ref<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.target_ref = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for target_ref: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<OperationRef> for super::OperationRef {
        type Error = super::error::ConversionError;
        fn try_from(
            value: OperationRef,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                generation: value.generation?,
                operation_id: value.operation_id?,
                receipt_ref: value.receipt_ref?,
                request_digest: value.request_digest?,
                target: value.target?,
                target_ref: value.target_ref?,
            })
        }
    }
    impl ::std::convert::From<super::OperationRef> for OperationRef {
        fn from(value: super::OperationRef) -> Self {
            Self {
                generation: Ok(value.generation),
                operation_id: Ok(value.operation_id),
                receipt_ref: Ok(value.receipt_ref),
                request_digest: Ok(value.request_digest),
                target: Ok(value.target),
                target_ref: Ok(value.target_ref),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct OutputChunk {
        offset: ::std::result::Result<u64, ::std::string::String>,
        stream: ::std::result::Result<super::OutputChunkStream, ::std::string::String>,
        text: ::std::result::Result<super::OutputChunkText, ::std::string::String>,
    }
    impl ::std::default::Default for OutputChunk {
        fn default() -> Self {
            Self {
                offset: Err("no value supplied for offset".to_string()),
                stream: Err("no value supplied for stream".to_string()),
                text: Err("no value supplied for text".to_string()),
            }
        }
    }
    impl OutputChunk {
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
            T: ::std::convert::TryInto<super::OutputChunkStream>,
            T::Error: ::std::fmt::Display,
        {
            self.stream = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for stream: {e}"));
            self
        }
        pub fn text<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OutputChunkText>,
            T::Error: ::std::fmt::Display,
        {
            self.text = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for text: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<OutputChunk> for super::OutputChunk {
        type Error = super::error::ConversionError;
        fn try_from(
            value: OutputChunk,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                offset: value.offset?,
                stream: value.stream?,
                text: value.text?,
            })
        }
    }
    impl ::std::convert::From<super::OutputChunk> for OutputChunk {
        fn from(value: super::OutputChunk) -> Self {
            Self {
                offset: Ok(value.offset),
                stream: Ok(value.stream),
                text: Ok(value.text),
            }
        }
    }
    #[derive(Clone)]
    pub struct PrepareSessionRequest {
        bindings: ::std::result::Result<Vec<super::PreparedBindingBundles>, ::std::string::String>,
        bundles: ::std::result::Result<Vec<super::BundleFetch>, ::std::string::String>,
        network: ::std::result::Result<super::NetworkCeiling, ::std::string::String>,
        resources: ::std::result::Result<super::ResourceCeiling, ::std::string::String>,
        root_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        secret_capability: ::std::result::Result<
            ::std::option::Option<super::SecretCapability>,
            ::std::string::String,
        >,
        session_id: ::std::result::Result<super::Identifier, ::std::string::String>,
    }
    impl ::std::default::Default for PrepareSessionRequest {
        fn default() -> Self {
            Self {
                bindings: Err("no value supplied for bindings".to_string()),
                bundles: Err("no value supplied for bundles".to_string()),
                network: Err("no value supplied for network".to_string()),
                resources: Err("no value supplied for resources".to_string()),
                root_id: Err("no value supplied for root_id".to_string()),
                secret_capability: Ok(Default::default()),
                session_id: Err("no value supplied for session_id".to_string()),
            }
        }
    }
    impl PrepareSessionRequest {
        pub fn bindings<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<Vec<super::PreparedBindingBundles>>,
            T::Error: ::std::fmt::Display,
        {
            self.bindings = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for bindings: {e}"));
            self
        }
        pub fn bundles<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<Vec<super::BundleFetch>>,
            T::Error: ::std::fmt::Display,
        {
            self.bundles = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for bundles: {e}"));
            self
        }
        pub fn network<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::NetworkCeiling>,
            T::Error: ::std::fmt::Display,
        {
            self.network = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for network: {e}"));
            self
        }
        pub fn resources<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ResourceCeiling>,
            T::Error: ::std::fmt::Display,
        {
            self.resources = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for resources: {e}"));
            self
        }
        pub fn root_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.root_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for root_id: {e}"));
            self
        }
        pub fn secret_capability<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::SecretCapability>>,
            T::Error: ::std::fmt::Display,
        {
            self.secret_capability = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for secret_capability: {e}"));
            self
        }
        pub fn session_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.session_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for session_id: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<PrepareSessionRequest> for super::PrepareSessionRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: PrepareSessionRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                bindings: value.bindings?,
                bundles: value.bundles?,
                network: value.network?,
                resources: value.resources?,
                root_id: value.root_id?,
                secret_capability: value.secret_capability?,
                session_id: value.session_id?,
            })
        }
    }
    impl ::std::convert::From<super::PrepareSessionRequest> for PrepareSessionRequest {
        fn from(value: super::PrepareSessionRequest) -> Self {
            Self {
                bindings: Ok(value.bindings),
                bundles: Ok(value.bundles),
                network: Ok(value.network),
                resources: Ok(value.resources),
                root_id: Ok(value.root_id),
                secret_capability: Ok(value.secret_capability),
                session_id: Ok(value.session_id),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PreparedBindingBundles {
        binding_ref: ::std::result::Result<super::Identifier, ::std::string::String>,
        bundle_digests: ::std::result::Result<Vec<super::Digest>, ::std::string::String>,
    }
    impl ::std::default::Default for PreparedBindingBundles {
        fn default() -> Self {
            Self {
                binding_ref: Err("no value supplied for binding_ref".to_string()),
                bundle_digests: Err("no value supplied for bundle_digests".to_string()),
            }
        }
    }
    impl PreparedBindingBundles {
        pub fn binding_ref<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.binding_ref = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for binding_ref: {e}"));
            self
        }
        pub fn bundle_digests<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<Vec<super::Digest>>,
            T::Error: ::std::fmt::Display,
        {
            self.bundle_digests = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for bundle_digests: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<PreparedBindingBundles> for super::PreparedBindingBundles {
        type Error = super::error::ConversionError;
        fn try_from(
            value: PreparedBindingBundles,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                binding_ref: value.binding_ref?,
                bundle_digests: value.bundle_digests?,
            })
        }
    }
    impl ::std::convert::From<super::PreparedBindingBundles> for PreparedBindingBundles {
        fn from(value: super::PreparedBindingBundles) -> Self {
            Self {
                binding_ref: Ok(value.binding_ref),
                bundle_digests: Ok(value.bundle_digests),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PreparedSession {
        preparation_ref: ::std::result::Result<super::Identifier, ::std::string::String>,
    }
    impl ::std::default::Default for PreparedSession {
        fn default() -> Self {
            Self {
                preparation_ref: Err("no value supplied for preparation_ref".to_string()),
            }
        }
    }
    impl PreparedSession {
        pub fn preparation_ref<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.preparation_ref = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for preparation_ref: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<PreparedSession> for super::PreparedSession {
        type Error = super::error::ConversionError;
        fn try_from(
            value: PreparedSession,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                preparation_ref: value.preparation_ref?,
            })
        }
    }
    impl ::std::convert::From<super::PreparedSession> for PreparedSession {
        fn from(value: super::PreparedSession) -> Self {
            Self {
                preparation_ref: Ok(value.preparation_ref),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ResolvedBinding {
        binding_ref: ::std::result::Result<super::Identifier, ::std::string::String>,
        capabilities: ::std::result::Result<Vec<super::HandCapability>, ::std::string::String>,
        hand_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        limits: ::std::result::Result<super::ResolvedBindingLimits, ::std::string::String>,
        realm: ::std::result::Result<super::ExecutionRealm, ::std::string::String>,
        recovery: ::std::result::Result<super::RecoveryClass, ::std::string::String>,
    }
    impl ::std::default::Default for ResolvedBinding {
        fn default() -> Self {
            Self {
                binding_ref: Err("no value supplied for binding_ref".to_string()),
                capabilities: Err("no value supplied for capabilities".to_string()),
                hand_id: Err("no value supplied for hand_id".to_string()),
                limits: Err("no value supplied for limits".to_string()),
                realm: Err("no value supplied for realm".to_string()),
                recovery: Err("no value supplied for recovery".to_string()),
            }
        }
    }
    impl ResolvedBinding {
        pub fn binding_ref<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.binding_ref = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for binding_ref: {e}"));
            self
        }
        pub fn capabilities<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<Vec<super::HandCapability>>,
            T::Error: ::std::fmt::Display,
        {
            self.capabilities = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for capabilities: {e}"));
            self
        }
        pub fn hand_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.hand_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for hand_id: {e}"));
            self
        }
        pub fn limits<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ResolvedBindingLimits>,
            T::Error: ::std::fmt::Display,
        {
            self.limits = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for limits: {e}"));
            self
        }
        pub fn realm<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ExecutionRealm>,
            T::Error: ::std::fmt::Display,
        {
            self.realm = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for realm: {e}"));
            self
        }
        pub fn recovery<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::RecoveryClass>,
            T::Error: ::std::fmt::Display,
        {
            self.recovery = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for recovery: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ResolvedBinding> for super::ResolvedBinding {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ResolvedBinding,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                binding_ref: value.binding_ref?,
                capabilities: value.capabilities?,
                hand_id: value.hand_id?,
                limits: value.limits?,
                realm: value.realm?,
                recovery: value.recovery?,
            })
        }
    }
    impl ::std::convert::From<super::ResolvedBinding> for ResolvedBinding {
        fn from(value: super::ResolvedBinding) -> Self {
            Self {
                binding_ref: Ok(value.binding_ref),
                capabilities: Ok(value.capabilities),
                hand_id: Ok(value.hand_id),
                limits: Ok(value.limits),
                realm: Ok(value.realm),
                recovery: Ok(value.recovery),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ResolvedBindingLimits {
        max_inline_input_bytes:
            ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
        max_inline_result_bytes:
            ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
        max_wait_ms: ::std::result::Result<u64, ::std::string::String>,
    }
    impl ::std::default::Default for ResolvedBindingLimits {
        fn default() -> Self {
            Self {
                max_inline_input_bytes: Err(
                    "no value supplied for max_inline_input_bytes".to_string()
                ),
                max_inline_result_bytes: Err(
                    "no value supplied for max_inline_result_bytes".to_string()
                ),
                max_wait_ms: Err("no value supplied for max_wait_ms".to_string()),
            }
        }
    }
    impl ResolvedBindingLimits {
        pub fn max_inline_input_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::num::NonZeroU64>,
            T::Error: ::std::fmt::Display,
        {
            self.max_inline_input_bytes = value.try_into().map_err(|e| {
                format!("error converting supplied value for max_inline_input_bytes: {e}")
            });
            self
        }
        pub fn max_inline_result_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::num::NonZeroU64>,
            T::Error: ::std::fmt::Display,
        {
            self.max_inline_result_bytes = value.try_into().map_err(|e| {
                format!("error converting supplied value for max_inline_result_bytes: {e}")
            });
            self
        }
        pub fn max_wait_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.max_wait_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for max_wait_ms: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ResolvedBindingLimits> for super::ResolvedBindingLimits {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ResolvedBindingLimits,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                max_inline_input_bytes: value.max_inline_input_bytes?,
                max_inline_result_bytes: value.max_inline_result_bytes?,
                max_wait_ms: value.max_wait_ms?,
            })
        }
    }
    impl ::std::convert::From<super::ResolvedBindingLimits> for ResolvedBindingLimits {
        fn from(value: super::ResolvedBindingLimits) -> Self {
            Self {
                max_inline_input_bytes: Ok(value.max_inline_input_bytes),
                max_inline_result_bytes: Ok(value.max_inline_result_bytes),
                max_wait_ms: Ok(value.max_wait_ms),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ResourceCeiling {
        max_output_bytes: ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
        timeout_ms: ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
    }
    impl ::std::default::Default for ResourceCeiling {
        fn default() -> Self {
            Self {
                max_output_bytes: Err("no value supplied for max_output_bytes".to_string()),
                timeout_ms: Err("no value supplied for timeout_ms".to_string()),
            }
        }
    }
    impl ResourceCeiling {
        pub fn max_output_bytes<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::num::NonZeroU64>,
            T::Error: ::std::fmt::Display,
        {
            self.max_output_bytes = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for max_output_bytes: {e}"));
            self
        }
        pub fn timeout_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::num::NonZeroU64>,
            T::Error: ::std::fmt::Display,
        {
            self.timeout_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for timeout_ms: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<ResourceCeiling> for super::ResourceCeiling {
        type Error = super::error::ConversionError;
        fn try_from(
            value: ResourceCeiling,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                max_output_bytes: value.max_output_bytes?,
                timeout_ms: value.timeout_ms?,
            })
        }
    }
    impl ::std::convert::From<super::ResourceCeiling> for ResourceCeiling {
        fn from(value: super::ResourceCeiling) -> Self {
            Self {
                max_output_bytes: Ok(value.max_output_bytes),
                timeout_ms: Ok(value.timeout_ms),
            }
        }
    }
    #[derive(Clone)]
    pub struct SandboxCopyRequest {
        direction: ::std::result::Result<super::SandboxCopyRequestDirection, ::std::string::String>,
        expected_generation: ::std::result::Result<super::Identifier, ::std::string::String>,
        object: ::std::result::Result<
            ::std::option::Option<super::ObjectReference>,
            ::std::string::String,
        >,
        operation_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        overwrite: ::std::result::Result<bool, ::std::string::String>,
        path: ::std::result::Result<super::SandboxCopyRequestPath, ::std::string::String>,
        request_digest: ::std::result::Result<super::Digest, ::std::string::String>,
        target: ::std::result::Result<super::SandboxTarget, ::std::string::String>,
        transfer: ::std::result::Result<super::ObjectTransferAuthority, ::std::string::String>,
    }
    impl ::std::default::Default for SandboxCopyRequest {
        fn default() -> Self {
            Self {
                direction: Err("no value supplied for direction".to_string()),
                expected_generation: Err("no value supplied for expected_generation".to_string()),
                object: Err("no value supplied for object".to_string()),
                operation_id: Err("no value supplied for operation_id".to_string()),
                overwrite: Err("no value supplied for overwrite".to_string()),
                path: Err("no value supplied for path".to_string()),
                request_digest: Err("no value supplied for request_digest".to_string()),
                target: Err("no value supplied for target".to_string()),
                transfer: Err("no value supplied for transfer".to_string()),
            }
        }
    }
    impl SandboxCopyRequest {
        pub fn direction<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SandboxCopyRequestDirection>,
            T::Error: ::std::fmt::Display,
        {
            self.direction = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for direction: {e}"));
            self
        }
        pub fn expected_generation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.expected_generation = value.try_into().map_err(|e| {
                format!("error converting supplied value for expected_generation: {e}")
            });
            self
        }
        pub fn object<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::ObjectReference>>,
            T::Error: ::std::fmt::Display,
        {
            self.object = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for object: {e}"));
            self
        }
        pub fn operation_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.operation_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for operation_id: {e}"));
            self
        }
        pub fn overwrite<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<bool>,
            T::Error: ::std::fmt::Display,
        {
            self.overwrite = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for overwrite: {e}"));
            self
        }
        pub fn path<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SandboxCopyRequestPath>,
            T::Error: ::std::fmt::Display,
        {
            self.path = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for path: {e}"));
            self
        }
        pub fn request_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Digest>,
            T::Error: ::std::fmt::Display,
        {
            self.request_digest = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for request_digest: {e}"));
            self
        }
        pub fn target<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SandboxTarget>,
            T::Error: ::std::fmt::Display,
        {
            self.target = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for target: {e}"));
            self
        }
        pub fn transfer<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ObjectTransferAuthority>,
            T::Error: ::std::fmt::Display,
        {
            self.transfer = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for transfer: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<SandboxCopyRequest> for super::SandboxCopyRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SandboxCopyRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                direction: value.direction?,
                expected_generation: value.expected_generation?,
                object: value.object?,
                operation_id: value.operation_id?,
                overwrite: value.overwrite?,
                path: value.path?,
                request_digest: value.request_digest?,
                target: value.target?,
                transfer: value.transfer?,
            })
        }
    }
    impl ::std::convert::From<super::SandboxCopyRequest> for SandboxCopyRequest {
        fn from(value: super::SandboxCopyRequest) -> Self {
            Self {
                direction: Ok(value.direction),
                expected_generation: Ok(value.expected_generation),
                object: Ok(value.object),
                operation_id: Ok(value.operation_id),
                overwrite: Ok(value.overwrite),
                path: Ok(value.path),
                request_digest: Ok(value.request_digest),
                target: Ok(value.target),
                transfer: Ok(value.transfer),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct SandboxCopyResult {
        file: ::std::result::Result<super::FileEntry, ::std::string::String>,
        object: ::std::result::Result<
            ::std::option::Option<super::ObjectReference>,
            ::std::string::String,
        >,
        operation_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        replayed: ::std::result::Result<bool, ::std::string::String>,
        request_digest: ::std::result::Result<super::Digest, ::std::string::String>,
    }
    impl ::std::default::Default for SandboxCopyResult {
        fn default() -> Self {
            Self {
                file: Err("no value supplied for file".to_string()),
                object: Err("no value supplied for object".to_string()),
                operation_id: Err("no value supplied for operation_id".to_string()),
                replayed: Err("no value supplied for replayed".to_string()),
                request_digest: Err("no value supplied for request_digest".to_string()),
            }
        }
    }
    impl SandboxCopyResult {
        pub fn file<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::FileEntry>,
            T::Error: ::std::fmt::Display,
        {
            self.file = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for file: {e}"));
            self
        }
        pub fn object<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::ObjectReference>>,
            T::Error: ::std::fmt::Display,
        {
            self.object = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for object: {e}"));
            self
        }
        pub fn operation_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.operation_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for operation_id: {e}"));
            self
        }
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
        pub fn request_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Digest>,
            T::Error: ::std::fmt::Display,
        {
            self.request_digest = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for request_digest: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<SandboxCopyResult> for super::SandboxCopyResult {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SandboxCopyResult,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                file: value.file?,
                object: value.object?,
                operation_id: value.operation_id?,
                replayed: value.replayed?,
                request_digest: value.request_digest?,
            })
        }
    }
    impl ::std::convert::From<super::SandboxCopyResult> for SandboxCopyResult {
        fn from(value: super::SandboxCopyResult) -> Self {
            Self {
                file: Ok(value.file),
                object: Ok(value.object),
                operation_id: Ok(value.operation_id),
                replayed: Ok(value.replayed),
                request_digest: Ok(value.request_digest),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct SandboxExecInput {
        command: ::std::result::Result<super::SandboxExecInputCommand, ::std::string::String>,
        cwd: ::std::result::Result<
            ::std::option::Option<super::SandboxExecInputCwd>,
            ::std::string::String,
        >,
        interactive: ::std::result::Result<bool, ::std::string::String>,
    }
    impl ::std::default::Default for SandboxExecInput {
        fn default() -> Self {
            Self {
                command: Err("no value supplied for command".to_string()),
                cwd: Ok(Default::default()),
                interactive: Err("no value supplied for interactive".to_string()),
            }
        }
    }
    impl SandboxExecInput {
        pub fn command<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SandboxExecInputCommand>,
            T::Error: ::std::fmt::Display,
        {
            self.command = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for command: {e}"));
            self
        }
        pub fn cwd<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::SandboxExecInputCwd>>,
            T::Error: ::std::fmt::Display,
        {
            self.cwd = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for cwd: {e}"));
            self
        }
        pub fn interactive<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<bool>,
            T::Error: ::std::fmt::Display,
        {
            self.interactive = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for interactive: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<SandboxExecInput> for super::SandboxExecInput {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SandboxExecInput,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                command: value.command?,
                cwd: value.cwd?,
                interactive: value.interactive?,
            })
        }
    }
    impl ::std::convert::From<super::SandboxExecInput> for SandboxExecInput {
        fn from(value: super::SandboxExecInput) -> Self {
            Self {
                command: Ok(value.command),
                cwd: Ok(value.cwd),
                interactive: Ok(value.interactive),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct SandboxExecutionRequest {
        execution_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        expected_generation: ::std::result::Result<super::Identifier, ::std::string::String>,
        input: ::std::result::Result<super::SandboxExecInput, ::std::string::String>,
        network: ::std::result::Result<super::NetworkCeiling, ::std::string::String>,
        request_digest: ::std::result::Result<super::Digest, ::std::string::String>,
        resources: ::std::result::Result<super::ResourceCeiling, ::std::string::String>,
        target: ::std::result::Result<super::SandboxTarget, ::std::string::String>,
    }
    impl ::std::default::Default for SandboxExecutionRequest {
        fn default() -> Self {
            Self {
                execution_id: Err("no value supplied for execution_id".to_string()),
                expected_generation: Err("no value supplied for expected_generation".to_string()),
                input: Err("no value supplied for input".to_string()),
                network: Err("no value supplied for network".to_string()),
                request_digest: Err("no value supplied for request_digest".to_string()),
                resources: Err("no value supplied for resources".to_string()),
                target: Err("no value supplied for target".to_string()),
            }
        }
    }
    impl SandboxExecutionRequest {
        pub fn execution_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.execution_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for execution_id: {e}"));
            self
        }
        pub fn expected_generation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.expected_generation = value.try_into().map_err(|e| {
                format!("error converting supplied value for expected_generation: {e}")
            });
            self
        }
        pub fn input<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SandboxExecInput>,
            T::Error: ::std::fmt::Display,
        {
            self.input = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for input: {e}"));
            self
        }
        pub fn network<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::NetworkCeiling>,
            T::Error: ::std::fmt::Display,
        {
            self.network = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for network: {e}"));
            self
        }
        pub fn request_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Digest>,
            T::Error: ::std::fmt::Display,
        {
            self.request_digest = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for request_digest: {e}"));
            self
        }
        pub fn resources<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ResourceCeiling>,
            T::Error: ::std::fmt::Display,
        {
            self.resources = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for resources: {e}"));
            self
        }
        pub fn target<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SandboxTarget>,
            T::Error: ::std::fmt::Display,
        {
            self.target = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for target: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<SandboxExecutionRequest> for super::SandboxExecutionRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SandboxExecutionRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                execution_id: value.execution_id?,
                expected_generation: value.expected_generation?,
                input: value.input?,
                network: value.network?,
                request_digest: value.request_digest?,
                resources: value.resources?,
                target: value.target?,
            })
        }
    }
    impl ::std::convert::From<super::SandboxExecutionRequest> for SandboxExecutionRequest {
        fn from(value: super::SandboxExecutionRequest) -> Self {
            Self {
                execution_id: Ok(value.execution_id),
                expected_generation: Ok(value.expected_generation),
                input: Ok(value.input),
                network: Ok(value.network),
                request_digest: Ok(value.request_digest),
                resources: Ok(value.resources),
                target: Ok(value.target),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct SandboxFileRequest {
        expected_generation: ::std::result::Result<super::Identifier, ::std::string::String>,
        path: ::std::result::Result<super::SandboxFileRequestPath, ::std::string::String>,
        target: ::std::result::Result<super::SandboxTarget, ::std::string::String>,
    }
    impl ::std::default::Default for SandboxFileRequest {
        fn default() -> Self {
            Self {
                expected_generation: Err("no value supplied for expected_generation".to_string()),
                path: Err("no value supplied for path".to_string()),
                target: Err("no value supplied for target".to_string()),
            }
        }
    }
    impl SandboxFileRequest {
        pub fn expected_generation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.expected_generation = value.try_into().map_err(|e| {
                format!("error converting supplied value for expected_generation: {e}")
            });
            self
        }
        pub fn path<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SandboxFileRequestPath>,
            T::Error: ::std::fmt::Display,
        {
            self.path = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for path: {e}"));
            self
        }
        pub fn target<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SandboxTarget>,
            T::Error: ::std::fmt::Display,
        {
            self.target = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for target: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<SandboxFileRequest> for super::SandboxFileRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SandboxFileRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                expected_generation: value.expected_generation?,
                path: value.path?,
                target: value.target?,
            })
        }
    }
    impl ::std::convert::From<super::SandboxFileRequest> for SandboxFileRequest {
        fn from(value: super::SandboxFileRequest) -> Self {
            Self {
                expected_generation: Ok(value.expected_generation),
                path: Ok(value.path),
                target: Ok(value.target),
            }
        }
    }
    #[derive(Clone)]
    pub struct SandboxFileWriteRequest {
        expected_generation: ::std::result::Result<super::Identifier, ::std::string::String>,
        operation_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        overwrite: ::std::result::Result<bool, ::std::string::String>,
        path: ::std::result::Result<super::SandboxFileWriteRequestPath, ::std::string::String>,
        request_digest: ::std::result::Result<super::Digest, ::std::string::String>,
        source: ::std::result::Result<super::SandboxFileWriteSource, ::std::string::String>,
        target: ::std::result::Result<super::SandboxTarget, ::std::string::String>,
    }
    impl ::std::default::Default for SandboxFileWriteRequest {
        fn default() -> Self {
            Self {
                expected_generation: Err("no value supplied for expected_generation".to_string()),
                operation_id: Err("no value supplied for operation_id".to_string()),
                overwrite: Err("no value supplied for overwrite".to_string()),
                path: Err("no value supplied for path".to_string()),
                request_digest: Err("no value supplied for request_digest".to_string()),
                source: Err("no value supplied for source".to_string()),
                target: Err("no value supplied for target".to_string()),
            }
        }
    }
    impl SandboxFileWriteRequest {
        pub fn expected_generation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.expected_generation = value.try_into().map_err(|e| {
                format!("error converting supplied value for expected_generation: {e}")
            });
            self
        }
        pub fn operation_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.operation_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for operation_id: {e}"));
            self
        }
        pub fn overwrite<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<bool>,
            T::Error: ::std::fmt::Display,
        {
            self.overwrite = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for overwrite: {e}"));
            self
        }
        pub fn path<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SandboxFileWriteRequestPath>,
            T::Error: ::std::fmt::Display,
        {
            self.path = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for path: {e}"));
            self
        }
        pub fn request_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Digest>,
            T::Error: ::std::fmt::Display,
        {
            self.request_digest = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for request_digest: {e}"));
            self
        }
        pub fn source<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SandboxFileWriteSource>,
            T::Error: ::std::fmt::Display,
        {
            self.source = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for source: {e}"));
            self
        }
        pub fn target<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SandboxTarget>,
            T::Error: ::std::fmt::Display,
        {
            self.target = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for target: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<SandboxFileWriteRequest> for super::SandboxFileWriteRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SandboxFileWriteRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                expected_generation: value.expected_generation?,
                operation_id: value.operation_id?,
                overwrite: value.overwrite?,
                path: value.path?,
                request_digest: value.request_digest?,
                source: value.source?,
                target: value.target?,
            })
        }
    }
    impl ::std::convert::From<super::SandboxFileWriteRequest> for SandboxFileWriteRequest {
        fn from(value: super::SandboxFileWriteRequest) -> Self {
            Self {
                expected_generation: Ok(value.expected_generation),
                operation_id: Ok(value.operation_id),
                overwrite: Ok(value.overwrite),
                path: Ok(value.path),
                request_digest: Ok(value.request_digest),
                source: Ok(value.source),
                target: Ok(value.target),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct SandboxFileWriteResult {
        file: ::std::result::Result<super::FileEntry, ::std::string::String>,
        operation_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        replayed: ::std::result::Result<bool, ::std::string::String>,
        request_digest: ::std::result::Result<super::Digest, ::std::string::String>,
    }
    impl ::std::default::Default for SandboxFileWriteResult {
        fn default() -> Self {
            Self {
                file: Err("no value supplied for file".to_string()),
                operation_id: Err("no value supplied for operation_id".to_string()),
                replayed: Err("no value supplied for replayed".to_string()),
                request_digest: Err("no value supplied for request_digest".to_string()),
            }
        }
    }
    impl SandboxFileWriteResult {
        pub fn file<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::FileEntry>,
            T::Error: ::std::fmt::Display,
        {
            self.file = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for file: {e}"));
            self
        }
        pub fn operation_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.operation_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for operation_id: {e}"));
            self
        }
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
        pub fn request_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Digest>,
            T::Error: ::std::fmt::Display,
        {
            self.request_digest = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for request_digest: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<SandboxFileWriteResult> for super::SandboxFileWriteResult {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SandboxFileWriteResult,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                file: value.file?,
                operation_id: value.operation_id?,
                replayed: value.replayed?,
                request_digest: value.request_digest?,
            })
        }
    }
    impl ::std::convert::From<super::SandboxFileWriteResult> for SandboxFileWriteResult {
        fn from(value: super::SandboxFileWriteResult) -> Self {
            Self {
                file: Ok(value.file),
                operation_id: Ok(value.operation_id),
                replayed: Ok(value.replayed),
                request_digest: Ok(value.request_digest),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct SandboxStatus {
        changed_at_ms: ::std::result::Result<::std::option::Option<u64>, ::std::string::String>,
        expires_at_ms: ::std::result::Result<
            ::std::option::Option<::std::num::NonZeroU64>,
            ::std::string::String,
        >,
        generation: ::std::result::Result<
            ::std::option::Option<super::SandboxStatusGeneration>,
            ::std::string::String,
        >,
        reason: ::std::result::Result<
            ::std::option::Option<super::SandboxStatusReason>,
            ::std::string::String,
        >,
        state: ::std::result::Result<super::SandboxState, ::std::string::String>,
        target: ::std::result::Result<super::SandboxTarget, ::std::string::String>,
        target_ref:
            ::std::result::Result<::std::option::Option<super::Identifier>, ::std::string::String>,
    }
    impl ::std::default::Default for SandboxStatus {
        fn default() -> Self {
            Self {
                changed_at_ms: Ok(Default::default()),
                expires_at_ms: Err("no value supplied for expires_at_ms".to_string()),
                generation: Ok(Default::default()),
                reason: Ok(Default::default()),
                state: Err("no value supplied for state".to_string()),
                target: Err("no value supplied for target".to_string()),
                target_ref: Ok(Default::default()),
            }
        }
    }
    impl SandboxStatus {
        pub fn changed_at_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<u64>>,
            T::Error: ::std::fmt::Display,
        {
            self.changed_at_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for changed_at_ms: {e}"));
            self
        }
        pub fn expires_at_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::std::num::NonZeroU64>>,
            T::Error: ::std::fmt::Display,
        {
            self.expires_at_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for expires_at_ms: {e}"));
            self
        }
        pub fn generation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::SandboxStatusGeneration>>,
            T::Error: ::std::fmt::Display,
        {
            self.generation = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for generation: {e}"));
            self
        }
        pub fn reason<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::SandboxStatusReason>>,
            T::Error: ::std::fmt::Display,
        {
            self.reason = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for reason: {e}"));
            self
        }
        pub fn state<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SandboxState>,
            T::Error: ::std::fmt::Display,
        {
            self.state = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for state: {e}"));
            self
        }
        pub fn target<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SandboxTarget>,
            T::Error: ::std::fmt::Display,
        {
            self.target = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for target: {e}"));
            self
        }
        pub fn target_ref<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::Identifier>>,
            T::Error: ::std::fmt::Display,
        {
            self.target_ref = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for target_ref: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<SandboxStatus> for super::SandboxStatus {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SandboxStatus,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                changed_at_ms: value.changed_at_ms?,
                expires_at_ms: value.expires_at_ms?,
                generation: value.generation?,
                reason: value.reason?,
                state: value.state?,
                target: value.target?,
                target_ref: value.target_ref?,
            })
        }
    }
    impl ::std::convert::From<super::SandboxStatus> for SandboxStatus {
        fn from(value: super::SandboxStatus) -> Self {
            Self {
                changed_at_ms: Ok(value.changed_at_ms),
                expires_at_ms: Ok(value.expires_at_ms),
                generation: Ok(value.generation),
                reason: Ok(value.reason),
                state: Ok(value.state),
                target: Ok(value.target),
                target_ref: Ok(value.target_ref),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct SandboxTarget {
        binding_ref: ::std::result::Result<super::Identifier, ::std::string::String>,
        kind: ::std::result::Result<super::TargetKind, ::std::string::String>,
        root_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        sandbox_id:
            ::std::result::Result<::std::option::Option<super::Identifier>, ::std::string::String>,
        session_id: ::std::result::Result<super::Identifier, ::std::string::String>,
    }
    impl ::std::default::Default for SandboxTarget {
        fn default() -> Self {
            Self {
                binding_ref: Err("no value supplied for binding_ref".to_string()),
                kind: Err("no value supplied for kind".to_string()),
                root_id: Err("no value supplied for root_id".to_string()),
                sandbox_id: Ok(Default::default()),
                session_id: Err("no value supplied for session_id".to_string()),
            }
        }
    }
    impl SandboxTarget {
        pub fn binding_ref<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.binding_ref = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for binding_ref: {e}"));
            self
        }
        pub fn kind<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::TargetKind>,
            T::Error: ::std::fmt::Display,
        {
            self.kind = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for kind: {e}"));
            self
        }
        pub fn root_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.root_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for root_id: {e}"));
            self
        }
        pub fn sandbox_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::Identifier>>,
            T::Error: ::std::fmt::Display,
        {
            self.sandbox_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for sandbox_id: {e}"));
            self
        }
        pub fn session_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.session_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for session_id: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<SandboxTarget> for super::SandboxTarget {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SandboxTarget,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                binding_ref: value.binding_ref?,
                kind: value.kind?,
                root_id: value.root_id?,
                sandbox_id: value.sandbox_id?,
                session_id: value.session_id?,
            })
        }
    }
    impl ::std::convert::From<super::SandboxTarget> for SandboxTarget {
        fn from(value: super::SandboxTarget) -> Self {
            Self {
                binding_ref: Ok(value.binding_ref),
                kind: Ok(value.kind),
                root_id: Ok(value.root_id),
                sandbox_id: Ok(value.sandbox_id),
                session_id: Ok(value.session_id),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct SealedBinding {
        binding_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        bundle: ::std::result::Result<
            ::std::option::Option<super::BundleDescriptor>,
            ::std::string::String,
        >,
        capability: ::std::result::Result<super::Identifier, ::std::string::String>,
        contract_digest: ::std::result::Result<super::Digest, ::std::string::String>,
        implementation_identity: ::std::result::Result<super::Digest, ::std::string::String>,
        policy_digest: ::std::result::Result<super::Digest, ::std::string::String>,
        realm: ::std::result::Result<super::ExecutionRealm, ::std::string::String>,
        realm_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        required_capabilities:
            ::std::result::Result<Vec<super::HandCapability>, ::std::string::String>,
        root_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        session_id: ::std::result::Result<super::Identifier, ::std::string::String>,
    }
    impl ::std::default::Default for SealedBinding {
        fn default() -> Self {
            Self {
                binding_id: Err("no value supplied for binding_id".to_string()),
                bundle: Ok(Default::default()),
                capability: Err("no value supplied for capability".to_string()),
                contract_digest: Err("no value supplied for contract_digest".to_string()),
                implementation_identity: Err(
                    "no value supplied for implementation_identity".to_string()
                ),
                policy_digest: Err("no value supplied for policy_digest".to_string()),
                realm: Err("no value supplied for realm".to_string()),
                realm_id: Err("no value supplied for realm_id".to_string()),
                required_capabilities: Ok(super::defaults::sealed_binding_required_capabilities()),
                root_id: Err("no value supplied for root_id".to_string()),
                session_id: Err("no value supplied for session_id".to_string()),
            }
        }
    }
    impl SealedBinding {
        pub fn binding_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.binding_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for binding_id: {e}"));
            self
        }
        pub fn bundle<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::BundleDescriptor>>,
            T::Error: ::std::fmt::Display,
        {
            self.bundle = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for bundle: {e}"));
            self
        }
        pub fn capability<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.capability = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for capability: {e}"));
            self
        }
        pub fn contract_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Digest>,
            T::Error: ::std::fmt::Display,
        {
            self.contract_digest = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for contract_digest: {e}"));
            self
        }
        pub fn implementation_identity<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Digest>,
            T::Error: ::std::fmt::Display,
        {
            self.implementation_identity = value.try_into().map_err(|e| {
                format!("error converting supplied value for implementation_identity: {e}")
            });
            self
        }
        pub fn policy_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Digest>,
            T::Error: ::std::fmt::Display,
        {
            self.policy_digest = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for policy_digest: {e}"));
            self
        }
        pub fn realm<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::ExecutionRealm>,
            T::Error: ::std::fmt::Display,
        {
            self.realm = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for realm: {e}"));
            self
        }
        pub fn realm_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.realm_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for realm_id: {e}"));
            self
        }
        pub fn required_capabilities<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<Vec<super::HandCapability>>,
            T::Error: ::std::fmt::Display,
        {
            self.required_capabilities = value.try_into().map_err(|e| {
                format!("error converting supplied value for required_capabilities: {e}")
            });
            self
        }
        pub fn root_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.root_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for root_id: {e}"));
            self
        }
        pub fn session_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.session_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for session_id: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<SealedBinding> for super::SealedBinding {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SealedBinding,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                binding_id: value.binding_id?,
                bundle: value.bundle?,
                capability: value.capability?,
                contract_digest: value.contract_digest?,
                implementation_identity: value.implementation_identity?,
                policy_digest: value.policy_digest?,
                realm: value.realm?,
                realm_id: value.realm_id?,
                required_capabilities: value.required_capabilities?,
                root_id: value.root_id?,
                session_id: value.session_id?,
            })
        }
    }
    impl ::std::convert::From<super::SealedBinding> for SealedBinding {
        fn from(value: super::SealedBinding) -> Self {
            Self {
                binding_id: Ok(value.binding_id),
                bundle: Ok(value.bundle),
                capability: Ok(value.capability),
                contract_digest: Ok(value.contract_digest),
                implementation_identity: Ok(value.implementation_identity),
                policy_digest: Ok(value.policy_digest),
                realm: Ok(value.realm),
                realm_id: Ok(value.realm_id),
                required_capabilities: Ok(value.required_capabilities),
                root_id: Ok(value.root_id),
                session_id: Ok(value.session_id),
            }
        }
    }
    #[derive(Clone)]
    pub struct SecretCapability {
        capability_ref: ::std::result::Result<super::Identifier, ::std::string::String>,
        env_names: ::std::result::Result<Vec<super::Identifier>, ::std::string::String>,
        expires_at_ms: ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
    }
    impl ::std::default::Default for SecretCapability {
        fn default() -> Self {
            Self {
                capability_ref: Err("no value supplied for capability_ref".to_string()),
                env_names: Err("no value supplied for env_names".to_string()),
                expires_at_ms: Err("no value supplied for expires_at_ms".to_string()),
            }
        }
    }
    impl SecretCapability {
        pub fn capability_ref<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.capability_ref = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for capability_ref: {e}"));
            self
        }
        pub fn env_names<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<Vec<super::Identifier>>,
            T::Error: ::std::fmt::Display,
        {
            self.env_names = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for env_names: {e}"));
            self
        }
        pub fn expires_at_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::num::NonZeroU64>,
            T::Error: ::std::fmt::Display,
        {
            self.expires_at_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for expires_at_ms: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<SecretCapability> for super::SecretCapability {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SecretCapability,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                capability_ref: value.capability_ref?,
                env_names: value.env_names?,
                expires_at_ms: value.expires_at_ms?,
            })
        }
    }
    impl ::std::convert::From<super::SecretCapability> for SecretCapability {
        fn from(value: super::SecretCapability) -> Self {
            Self {
                capability_ref: Ok(value.capability_ref),
                env_names: Ok(value.env_names),
                expires_at_ms: Ok(value.expires_at_ms),
            }
        }
    }
    #[derive(Clone)]
    pub struct SecretDeliveryRequest {
        capability_ref: ::std::result::Result<super::Identifier, ::std::string::String>,
        generation_intent: ::std::result::Result<super::Identifier, ::std::string::String>,
        hand_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        root_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        session_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        target: ::std::result::Result<super::SandboxTarget, ::std::string::String>,
    }
    impl ::std::default::Default for SecretDeliveryRequest {
        fn default() -> Self {
            Self {
                capability_ref: Err("no value supplied for capability_ref".to_string()),
                generation_intent: Err("no value supplied for generation_intent".to_string()),
                hand_id: Err("no value supplied for hand_id".to_string()),
                root_id: Err("no value supplied for root_id".to_string()),
                session_id: Err("no value supplied for session_id".to_string()),
                target: Err("no value supplied for target".to_string()),
            }
        }
    }
    impl SecretDeliveryRequest {
        pub fn capability_ref<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.capability_ref = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for capability_ref: {e}"));
            self
        }
        pub fn generation_intent<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.generation_intent = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for generation_intent: {e}"));
            self
        }
        pub fn hand_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.hand_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for hand_id: {e}"));
            self
        }
        pub fn root_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.root_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for root_id: {e}"));
            self
        }
        pub fn session_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.session_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for session_id: {e}"));
            self
        }
        pub fn target<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SandboxTarget>,
            T::Error: ::std::fmt::Display,
        {
            self.target = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for target: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<SecretDeliveryRequest> for super::SecretDeliveryRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SecretDeliveryRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                capability_ref: value.capability_ref?,
                generation_intent: value.generation_intent?,
                hand_id: value.hand_id?,
                root_id: value.root_id?,
                session_id: value.session_id?,
                target: value.target?,
            })
        }
    }
    impl ::std::convert::From<super::SecretDeliveryRequest> for SecretDeliveryRequest {
        fn from(value: super::SecretDeliveryRequest) -> Self {
            Self {
                capability_ref: Ok(value.capability_ref),
                generation_intent: Ok(value.generation_intent),
                hand_id: Ok(value.hand_id),
                root_id: Ok(value.root_id),
                session_id: Ok(value.session_id),
                target: Ok(value.target),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct SubmitReceipt {
        observation: ::std::result::Result<super::OperationObservation, ::std::string::String>,
        operation: ::std::result::Result<super::OperationRef, ::std::string::String>,
        replayed: ::std::result::Result<bool, ::std::string::String>,
    }
    impl ::std::default::Default for SubmitReceipt {
        fn default() -> Self {
            Self {
                observation: Err("no value supplied for observation".to_string()),
                operation: Err("no value supplied for operation".to_string()),
                replayed: Err("no value supplied for replayed".to_string()),
            }
        }
    }
    impl SubmitReceipt {
        pub fn observation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationObservation>,
            T::Error: ::std::fmt::Display,
        {
            self.observation = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for observation: {e}"));
            self
        }
        pub fn operation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationRef>,
            T::Error: ::std::fmt::Display,
        {
            self.operation = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for operation: {e}"));
            self
        }
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
    }
    impl ::std::convert::TryFrom<SubmitReceipt> for super::SubmitReceipt {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SubmitReceipt,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                observation: value.observation?,
                operation: value.operation?,
                replayed: value.replayed?,
            })
        }
    }
    impl ::std::convert::From<super::SubmitReceipt> for SubmitReceipt {
        fn from(value: super::SubmitReceipt) -> Self {
            Self {
                observation: Ok(value.observation),
                operation: Ok(value.operation),
                replayed: Ok(value.replayed),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct SubmitRequest {
        envelope: ::std::result::Result<super::OperationEnvelope, ::std::string::String>,
        wait_up_to_ms: ::std::result::Result<u64, ::std::string::String>,
    }
    impl ::std::default::Default for SubmitRequest {
        fn default() -> Self {
            Self {
                envelope: Err("no value supplied for envelope".to_string()),
                wait_up_to_ms: Err("no value supplied for wait_up_to_ms".to_string()),
            }
        }
    }
    impl SubmitRequest {
        pub fn envelope<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationEnvelope>,
            T::Error: ::std::fmt::Display,
        {
            self.envelope = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for envelope: {e}"));
            self
        }
        pub fn wait_up_to_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<u64>,
            T::Error: ::std::fmt::Display,
        {
            self.wait_up_to_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for wait_up_to_ms: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<SubmitRequest> for super::SubmitRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: SubmitRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                envelope: value.envelope?,
                wait_up_to_ms: value.wait_up_to_ms?,
            })
        }
    }
    impl ::std::convert::From<super::SubmitRequest> for SubmitRequest {
        fn from(value: super::SubmitRequest) -> Self {
            Self {
                envelope: Ok(value.envelope),
                wait_up_to_ms: Ok(value.wait_up_to_ms),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct TargetReceipt {
        expires_at_ms: ::std::result::Result<::std::num::NonZeroU64, ::std::string::String>,
        generation: ::std::result::Result<super::Identifier, ::std::string::String>,
        target_ref: ::std::result::Result<super::Identifier, ::std::string::String>,
    }
    impl ::std::default::Default for TargetReceipt {
        fn default() -> Self {
            Self {
                expires_at_ms: Err("no value supplied for expires_at_ms".to_string()),
                generation: Err("no value supplied for generation".to_string()),
                target_ref: Err("no value supplied for target_ref".to_string()),
            }
        }
    }
    impl TargetReceipt {
        pub fn expires_at_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::num::NonZeroU64>,
            T::Error: ::std::fmt::Display,
        {
            self.expires_at_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for expires_at_ms: {e}"));
            self
        }
        pub fn generation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.generation = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for generation: {e}"));
            self
        }
        pub fn target_ref<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.target_ref = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for target_ref: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<TargetReceipt> for super::TargetReceipt {
        type Error = super::error::ConversionError;
        fn try_from(
            value: TargetReceipt,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                expires_at_ms: value.expires_at_ms?,
                generation: value.generation?,
                target_ref: value.target_ref?,
            })
        }
    }
    impl ::std::convert::From<super::TargetReceipt> for TargetReceipt {
        fn from(value: super::TargetReceipt) -> Self {
            Self {
                expires_at_ms: Ok(value.expires_at_ms),
                generation: Ok(value.generation),
                target_ref: Ok(value.target_ref),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct TerminalResult {
        duration_ms: ::std::result::Result<::std::option::Option<u64>, ::std::string::String>,
        exit_code: ::std::result::Result<::std::option::Option<i64>, ::std::string::String>,
        inline: ::std::result::Result<
            ::std::option::Option<::serde_json::Value>,
            ::std::string::String,
        >,
        is_error: ::std::result::Result<bool, ::std::string::String>,
        object: ::std::result::Result<
            ::std::option::Option<super::ObjectReference>,
            ::std::string::String,
        >,
        outcome: ::std::result::Result<super::TerminalOutcome, ::std::string::String>,
        terminal_digest: ::std::result::Result<super::Digest, ::std::string::String>,
    }
    impl ::std::default::Default for TerminalResult {
        fn default() -> Self {
            Self {
                duration_ms: Ok(Default::default()),
                exit_code: Ok(Default::default()),
                inline: Ok(Default::default()),
                is_error: Err("no value supplied for is_error".to_string()),
                object: Ok(Default::default()),
                outcome: Err("no value supplied for outcome".to_string()),
                terminal_digest: Err("no value supplied for terminal_digest".to_string()),
            }
        }
    }
    impl TerminalResult {
        pub fn duration_ms<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<u64>>,
            T::Error: ::std::fmt::Display,
        {
            self.duration_ms = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for duration_ms: {e}"));
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
        pub fn inline<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<::serde_json::Value>>,
            T::Error: ::std::fmt::Display,
        {
            self.inline = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for inline: {e}"));
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
        pub fn object<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<::std::option::Option<super::ObjectReference>>,
            T::Error: ::std::fmt::Display,
        {
            self.object = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for object: {e}"));
            self
        }
        pub fn outcome<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::TerminalOutcome>,
            T::Error: ::std::fmt::Display,
        {
            self.outcome = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for outcome: {e}"));
            self
        }
        pub fn terminal_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Digest>,
            T::Error: ::std::fmt::Display,
        {
            self.terminal_digest = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for terminal_digest: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<TerminalResult> for super::TerminalResult {
        type Error = super::error::ConversionError;
        fn try_from(
            value: TerminalResult,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                duration_ms: value.duration_ms?,
                exit_code: value.exit_code?,
                inline: value.inline?,
                is_error: value.is_error?,
                object: value.object?,
                outcome: value.outcome?,
                terminal_digest: value.terminal_digest?,
            })
        }
    }
    impl ::std::convert::From<super::TerminalResult> for TerminalResult {
        fn from(value: super::TerminalResult) -> Self {
            Self {
                duration_ms: Ok(value.duration_ms),
                exit_code: Ok(value.exit_code),
                inline: Ok(value.inline),
                is_error: Ok(value.is_error),
                object: Ok(value.object),
                outcome: Ok(value.outcome),
                terminal_digest: Ok(value.terminal_digest),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct WriteStdinReceipt {
        accepted: ::std::result::Result<bool, ::std::string::String>,
        observation: ::std::result::Result<super::OperationObservation, ::std::string::String>,
        operation_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        replayed: ::std::result::Result<bool, ::std::string::String>,
        request_digest: ::std::result::Result<super::Digest, ::std::string::String>,
    }
    impl ::std::default::Default for WriteStdinReceipt {
        fn default() -> Self {
            Self {
                accepted: Err("no value supplied for accepted".to_string()),
                observation: Err("no value supplied for observation".to_string()),
                operation_id: Err("no value supplied for operation_id".to_string()),
                replayed: Err("no value supplied for replayed".to_string()),
                request_digest: Err("no value supplied for request_digest".to_string()),
            }
        }
    }
    impl WriteStdinReceipt {
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
        pub fn observation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::OperationObservation>,
            T::Error: ::std::fmt::Display,
        {
            self.observation = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for observation: {e}"));
            self
        }
        pub fn operation_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.operation_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for operation_id: {e}"));
            self
        }
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
        pub fn request_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Digest>,
            T::Error: ::std::fmt::Display,
        {
            self.request_digest = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for request_digest: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<WriteStdinReceipt> for super::WriteStdinReceipt {
        type Error = super::error::ConversionError;
        fn try_from(
            value: WriteStdinReceipt,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                accepted: value.accepted?,
                observation: value.observation?,
                operation_id: value.operation_id?,
                replayed: value.replayed?,
                request_digest: value.request_digest?,
            })
        }
    }
    impl ::std::convert::From<super::WriteStdinReceipt> for WriteStdinReceipt {
        fn from(value: super::WriteStdinReceipt) -> Self {
            Self {
                accepted: Ok(value.accepted),
                observation: Ok(value.observation),
                operation_id: Ok(value.operation_id),
                replayed: Ok(value.replayed),
                request_digest: Ok(value.request_digest),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct WriteStdinRequest {
        eof: ::std::result::Result<bool, ::std::string::String>,
        execution_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        expected_generation: ::std::result::Result<super::Identifier, ::std::string::String>,
        operation_id: ::std::result::Result<super::Identifier, ::std::string::String>,
        request_digest: ::std::result::Result<super::Digest, ::std::string::String>,
        target: ::std::result::Result<super::SandboxTarget, ::std::string::String>,
        text: ::std::result::Result<super::WriteStdinRequestText, ::std::string::String>,
    }
    impl ::std::default::Default for WriteStdinRequest {
        fn default() -> Self {
            Self {
                eof: Err("no value supplied for eof".to_string()),
                execution_id: Err("no value supplied for execution_id".to_string()),
                expected_generation: Err("no value supplied for expected_generation".to_string()),
                operation_id: Err("no value supplied for operation_id".to_string()),
                request_digest: Err("no value supplied for request_digest".to_string()),
                target: Err("no value supplied for target".to_string()),
                text: Err("no value supplied for text".to_string()),
            }
        }
    }
    impl WriteStdinRequest {
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
        pub fn execution_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.execution_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for execution_id: {e}"));
            self
        }
        pub fn expected_generation<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.expected_generation = value.try_into().map_err(|e| {
                format!("error converting supplied value for expected_generation: {e}")
            });
            self
        }
        pub fn operation_id<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Identifier>,
            T::Error: ::std::fmt::Display,
        {
            self.operation_id = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for operation_id: {e}"));
            self
        }
        pub fn request_digest<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::Digest>,
            T::Error: ::std::fmt::Display,
        {
            self.request_digest = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for request_digest: {e}"));
            self
        }
        pub fn target<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::SandboxTarget>,
            T::Error: ::std::fmt::Display,
        {
            self.target = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for target: {e}"));
            self
        }
        pub fn text<T>(mut self, value: T) -> Self
        where
            T: ::std::convert::TryInto<super::WriteStdinRequestText>,
            T::Error: ::std::fmt::Display,
        {
            self.text = value
                .try_into()
                .map_err(|e| format!("error converting supplied value for text: {e}"));
            self
        }
    }
    impl ::std::convert::TryFrom<WriteStdinRequest> for super::WriteStdinRequest {
        type Error = super::error::ConversionError;
        fn try_from(
            value: WriteStdinRequest,
        ) -> ::std::result::Result<Self, super::error::ConversionError> {
            Ok(Self {
                eof: value.eof?,
                execution_id: value.execution_id?,
                expected_generation: value.expected_generation?,
                operation_id: value.operation_id?,
                request_digest: value.request_digest?,
                target: value.target?,
                text: value.text?,
            })
        }
    }
    impl ::std::convert::From<super::WriteStdinRequest> for WriteStdinRequest {
        fn from(value: super::WriteStdinRequest) -> Self {
            Self {
                eof: Ok(value.eof),
                execution_id: Ok(value.execution_id),
                expected_generation: Ok(value.expected_generation),
                operation_id: Ok(value.operation_id),
                request_digest: Ok(value.request_digest),
                target: Ok(value.target),
                text: Ok(value.text),
            }
        }
    }
}
#[doc = r" Generation of default values for serde."]
pub mod defaults {
    pub(super) fn sealed_binding_required_capabilities() -> Vec<super::HandCapability> {
        vec![super::HandCapability::Execution]
    }
}
