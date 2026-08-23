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
#[doc = "Brain invoking the loop. One activation per lifecycle point; the message activation drives one turn."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Brain invoking the loop. One activation per lifecycle point; the message activation drives one turn.\","]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"activation_id\","]
#[doc = "        \"kind\","]
#[doc = "        \"kv\","]
#[doc = "        \"resumed\","]
#[doc = "        \"session\","]
#[doc = "        \"tail\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"activation_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/Identifier\""]
#[doc = "        },"]
#[doc = "        \"inherited\": {"]
#[doc = "          \"description\": \"Sealed inherited context from a context fork, in order, preceding the tail. Delivered only on a child session's fresh hydration (no own context floor); the messages are parent history, not child journal entries, so they carry no seqs and are never covered by marks.\","]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"$ref\": \"#/$defs/ModelMessage\""]
#[doc = "          },"]
#[doc = "          \"maxItems\": 512"]
#[doc = "        },"]
#[doc = "        \"kind\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"session_start\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"kv\": {"]
#[doc = "          \"$ref\": \"#/$defs/JsonObject\""]
#[doc = "        },"]
#[doc = "        \"latest_mark\": {"]
#[doc = "          \"$ref\": \"#/$defs/MarkView\""]
#[doc = "        },"]
#[doc = "        \"resumed\": {"]
#[doc = "          \"type\": \"boolean\""]
#[doc = "        },"]
#[doc = "        \"session\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionContextView\""]
#[doc = "        },"]
#[doc = "        \"tail\": {"]
#[doc = "          \"description\": \"Journal entries after the latest mark (bounded; truncated_tail is set when the bound cut it). The kernel's own checkpoint-plus-tail hydration shape, pushed as data.\","]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"$ref\": \"#/$defs/JournalEntryView\""]
#[doc = "          },"]
#[doc = "          \"maxItems\": 512"]
#[doc = "        },"]
#[doc = "        \"truncated_tail\": {"]
#[doc = "          \"type\": \"boolean\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"activation_id\","]
#[doc = "        \"kind\","]
#[doc = "        \"message\","]
#[doc = "        \"session\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"activation_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/Identifier\""]
#[doc = "        },"]
#[doc = "        \"kind\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"message\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"message\": {"]
#[doc = "          \"type\": \"object\","]
#[doc = "          \"required\": ["]
#[doc = "            \"at\","]
#[doc = "            \"content\","]
#[doc = "            \"seq\""]
#[doc = "          ],"]
#[doc = "          \"properties\": {"]
#[doc = "            \"at\": {"]
#[doc = "              \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "            },"]
#[doc = "            \"content\": {"]
#[doc = "              \"type\": \"array\","]
#[doc = "              \"items\": {"]
#[doc = "                \"$ref\": \"#/$defs/ContentView\""]
#[doc = "              },"]
#[doc = "              \"maxItems\": 64"]
#[doc = "            },"]
#[doc = "            \"seq\": {"]
#[doc = "              \"$ref\": \"#/$defs/Seq\""]
#[doc = "            }"]
#[doc = "          },"]
#[doc = "          \"additionalProperties\": false"]
#[doc = "        },"]
#[doc = "        \"session\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionContextView\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"description\": \"Best-effort; the kernel does not wait on it beyond a short budget.\","]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"activation_id\","]
#[doc = "        \"kind\","]
#[doc = "        \"session\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"activation_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/Identifier\""]
#[doc = "        },"]
#[doc = "        \"kind\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"session_end\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"session\": {"]
#[doc = "          \"$ref\": \"#/$defs/SessionContextView\""]
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
pub enum ActivationRequest {
    #[serde(rename = "session_start")]
    SessionStart {
        activation_id: Identifier,
        #[doc = "Sealed inherited context from a context fork, in order, preceding the tail. Delivered only on a child session's fresh hydration (no own context floor); the messages are parent history, not child journal entries, so they carry no seqs and are never covered by marks."]
        #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
        inherited: ::std::vec::Vec<ModelMessage>,
        kv: JsonObject,
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        latest_mark: ::std::option::Option<MarkView>,
        resumed: bool,
        session: SessionContextView,
        #[doc = "Journal entries after the latest mark (bounded; truncated_tail is set when the bound cut it). The kernel's own checkpoint-plus-tail hydration shape, pushed as data."]
        tail: ::std::vec::Vec<JournalEntryView>,
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        truncated_tail: ::std::option::Option<bool>,
    },
    #[serde(rename = "message")]
    Message {
        activation_id: Identifier,
        message: ActivationRequestMessage,
        session: SessionContextView,
    },
    #[doc = "Best-effort; the kernel does not wait on it beyond a short budget."]
    #[serde(rename = "session_end")]
    SessionEnd {
        activation_id: Identifier,
        session: SessionContextView,
    },
}
#[doc = "`ActivationRequestMessage`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"at\","]
#[doc = "    \"content\","]
#[doc = "    \"seq\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"at\": {"]
#[doc = "      \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "    },"]
#[doc = "    \"content\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/ContentView\""]
#[doc = "      },"]
#[doc = "      \"maxItems\": 64"]
#[doc = "    },"]
#[doc = "    \"seq\": {"]
#[doc = "      \"$ref\": \"#/$defs/Seq\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ActivationRequestMessage {
    pub at: Timestamp,
    pub content: ::std::vec::Vec<ContentView>,
    pub seq: Seq,
}
#[doc = "How the activation itself ended. Turn outcomes travel through turn_finish/turn_fail ops; a message activation that ends without a terminal op is a loop defect and interrupts the turn."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"How the activation itself ended. Turn outcomes travel through turn_finish/turn_fail ops; a message activation that ends without a terminal op is a loop defect and interrupts the turn.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"activation_id\","]
#[doc = "    \"outcome\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"activation_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/Identifier\""]
#[doc = "    },"]
#[doc = "    \"error\": {"]
#[doc = "      \"$ref\": \"#/$defs/AgentloopError\""]
#[doc = "    },"]
#[doc = "    \"outcome\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"completed\","]
#[doc = "        \"failed\","]
#[doc = "        \"aborted\""]
#[doc = "      ]"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ActivationResult {
    pub activation_id: Identifier,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub error: ::std::option::Option<AgentloopError>,
    pub outcome: ActivationResultOutcome,
}
#[doc = "`ActivationResultOutcome`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"completed\","]
#[doc = "    \"failed\","]
#[doc = "    \"aborted\""]
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
pub enum ActivationResultOutcome {
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "aborted")]
    Aborted,
}
impl ::std::fmt::Display for ActivationResultOutcome {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Completed => f.write_str("completed"),
            Self::Failed => f.write_str("failed"),
            Self::Aborted => f.write_str("aborted"),
        }
    }
}
impl ::std::str::FromStr for ActivationResultOutcome {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "aborted" => Ok(Self::Aborted),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ActivationResultOutcome {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ActivationResultOutcome {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ActivationResultOutcome {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`AgentloopError`"]
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
#[doc = "      \"$ref\": \"#/$defs/AgentloopErrorCode\""]
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
pub struct AgentloopError {
    pub code: AgentloopErrorCode,
    #[serde(default, skip_serializing_if = "::serde_json::Map::is_empty")]
    pub details: ::serde_json::Map<::std::string::String, ::serde_json::Value>,
    pub message: AgentloopErrorMessage,
    pub retryable: bool,
}
#[doc = "`AgentloopErrorCode`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"invalid_request\","]
#[doc = "    \"unsealed_tool\","]
#[doc = "    \"turn_already_terminal\","]
#[doc = "    \"budget_exceeded\","]
#[doc = "    \"entry_too_large\","]
#[doc = "    \"kv_limit\","]
#[doc = "    \"provider_error\","]
#[doc = "    \"tool_error\","]
#[doc = "    \"aborted\","]
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
pub enum AgentloopErrorCode {
    #[serde(rename = "invalid_request")]
    InvalidRequest,
    #[serde(rename = "unsealed_tool")]
    UnsealedTool,
    #[serde(rename = "turn_already_terminal")]
    TurnAlreadyTerminal,
    #[serde(rename = "budget_exceeded")]
    BudgetExceeded,
    #[serde(rename = "entry_too_large")]
    EntryTooLarge,
    #[serde(rename = "kv_limit")]
    KvLimit,
    #[serde(rename = "provider_error")]
    ProviderError,
    #[serde(rename = "tool_error")]
    ToolError,
    #[serde(rename = "aborted")]
    Aborted,
    #[serde(rename = "internal")]
    Internal,
}
impl ::std::fmt::Display for AgentloopErrorCode {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::InvalidRequest => f.write_str("invalid_request"),
            Self::UnsealedTool => f.write_str("unsealed_tool"),
            Self::TurnAlreadyTerminal => f.write_str("turn_already_terminal"),
            Self::BudgetExceeded => f.write_str("budget_exceeded"),
            Self::EntryTooLarge => f.write_str("entry_too_large"),
            Self::KvLimit => f.write_str("kv_limit"),
            Self::ProviderError => f.write_str("provider_error"),
            Self::ToolError => f.write_str("tool_error"),
            Self::Aborted => f.write_str("aborted"),
            Self::Internal => f.write_str("internal"),
        }
    }
}
impl ::std::str::FromStr for AgentloopErrorCode {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "invalid_request" => Ok(Self::InvalidRequest),
            "unsealed_tool" => Ok(Self::UnsealedTool),
            "turn_already_terminal" => Ok(Self::TurnAlreadyTerminal),
            "budget_exceeded" => Ok(Self::BudgetExceeded),
            "entry_too_large" => Ok(Self::EntryTooLarge),
            "kv_limit" => Ok(Self::KvLimit),
            "provider_error" => Ok(Self::ProviderError),
            "tool_error" => Ok(Self::ToolError),
            "aborted" => Ok(Self::Aborted),
            "internal" => Ok(Self::Internal),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for AgentloopErrorCode {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for AgentloopErrorCode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for AgentloopErrorCode {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`AgentloopErrorMessage`"]
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
pub struct AgentloopErrorMessage(::std::string::String);
impl ::std::ops::Deref for AgentloopErrorMessage {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<AgentloopErrorMessage> for ::std::string::String {
    fn from(value: AgentloopErrorMessage) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for AgentloopErrorMessage {
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
impl ::std::convert::TryFrom<&str> for AgentloopErrorMessage {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for AgentloopErrorMessage {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for AgentloopErrorMessage {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for AgentloopErrorMessage {
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
#[doc = "Which agentloop a session runs. Sealed at session creation; children default to the parent's selector and may override at spawn. The sealed identity of a custom loop is (source_bundle_sha256, toolchain): componentization is non-deterministic, so the deterministic esbuild source bundle is the identity and Brain-side tooling componentizes it, cached by this pair."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Which agentloop a session runs. Sealed at session creation; children default to the parent's selector and may override at spawn. The sealed identity of a custom loop is (source_bundle_sha256, toolchain): componentization is non-deterministic, so the deterministic esbuild source bundle is the identity and Brain-side tooling componentizes it, cached by this pair.\","]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"kind\","]
#[doc = "        \"name\","]
#[doc = "        \"version\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"kind\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"official\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"name\": {"]
#[doc = "          \"description\": \"An official loop name, e.g. \\\"aex\\\", \\\"pi\\\", \\\"codex-style\\\". Officials are prebuilt and stored by the composition; no bytes travel at create.\","]
#[doc = "          \"$ref\": \"#/$defs/Identifier\""]
#[doc = "        },"]
#[doc = "        \"version\": {"]
#[doc = "          \"$ref\": \"#/$defs/Identifier\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"kind\","]
#[doc = "        \"source_bundle_bytes\","]
#[doc = "        \"source_bundle_sha256\","]
#[doc = "        \"toolchain\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"kind\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"custom\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"source_bundle_bytes\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"maximum\": 8388608.0,"]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"source_bundle_sha256\": {"]
#[doc = "          \"description\": \"SHA-256 of the customer's deterministic esbuild source bundle.\","]
#[doc = "          \"$ref\": \"#/$defs/Digest\""]
#[doc = "        },"]
#[doc = "        \"toolchain\": {"]
#[doc = "          \"description\": \"The pinned loop-toolchain identity (engine + componentizer + wasmtime config family) the composition builds and runs this bundle with.\","]
#[doc = "          \"$ref\": \"#/$defs/Identifier\""]
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
pub enum AgentloopSelector {
    #[serde(rename = "official")]
    Official {
        #[doc = "An official loop name, e.g. \"aex\", \"pi\", \"codex-style\". Officials are prebuilt and stored by the composition; no bytes travel at create."]
        name: Identifier,
        version: Identifier,
    },
    #[serde(rename = "custom")]
    Custom {
        source_bundle_bytes: ::std::num::NonZeroU64,
        #[doc = "SHA-256 of the customer's deterministic esbuild source bundle."]
        source_bundle_sha256: Digest,
        #[doc = "The pinned loop-toolchain identity (engine + componentizer + wasmtime config family) the composition builds and runs this bundle with."]
        toolchain: Identifier,
    },
}
#[doc = "One folded model round. Token deltas stream Brain-to-application directly; the loop receives complete messages only."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"One folded model round. Token deltas stream Brain-to-application directly; the loop receives complete messages only.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"content\","]
#[doc = "    \"model\","]
#[doc = "    \"stop_reason\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"content\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/ContentView\""]
#[doc = "      },"]
#[doc = "      \"maxItems\": 64"]
#[doc = "    },"]
#[doc = "    \"model\": {"]
#[doc = "      \"$ref\": \"#/$defs/ModelName\""]
#[doc = "    },"]
#[doc = "    \"stop_reason\": {"]
#[doc = "      \"$ref\": \"#/$defs/ModelStopReason\""]
#[doc = "    },"]
#[doc = "    \"usage\": {"]
#[doc = "      \"$ref\": \"#/$defs/UsageView\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct AssistantMessageView {
    pub content: ::std::vec::Vec<ContentView>,
    pub model: ModelName,
    pub stop_reason: ModelStopReason,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub usage: ::std::option::Option<UsageView>,
}
#[doc = "The single current, transport-neutral Brain to loop-host agentloop contract. An agentloop is capability-pure policy code driving a session's turns; Brain executes and journals every effect. The canonical schema digest is the compatibility identity; the wire carries no protocol version. Delivery is at-least-once: Brain and the loop host deduplicate ctx operations by (op_id, canonical request digest); reusing an op_id with a different digest is a permanent conflict. Loss of an activation mid-turn interrupts that turn honestly; the session survives and rehydrates from kv and the latest mark."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"$id\": \"https://github.com/aexhq/brain/contracts/agentloop/v1/contract.json\","]
#[doc = "  \"title\": \"BrainAgentloopContract\","]
#[doc = "  \"description\": \"The single current, transport-neutral Brain to loop-host agentloop contract. An agentloop is capability-pure policy code driving a session's turns; Brain executes and journals every effect. The canonical schema digest is the compatibility identity; the wire carries no protocol version. Delivery is at-least-once: Brain and the loop host deduplicate ctx operations by (op_id, canonical request digest); reusing an op_id with a different digest is a permanent conflict. Loss of an activation mid-turn interrupts that turn honestly; the session survives and rehydrates from kv and the latest mark.\","]
#[doc = "  \"required\": ["]
#[doc = "    \"contract\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"contract\": {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"activations\","]
#[doc = "        \"ops\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"activations\": {"]
#[doc = "          \"const\": ["]
#[doc = "            \"session_start\","]
#[doc = "            \"message\","]
#[doc = "            \"session_end\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"ops\": {"]
#[doc = "          \"const\": ["]
#[doc = "            \"model_stream\","]
#[doc = "            \"tools_dispatch\","]
#[doc = "            \"journal_append\","]
#[doc = "            \"kv_get\","]
#[doc = "            \"kv_set\","]
#[doc = "            \"journal_read\","]
#[doc = "            \"turn_finish\","]
#[doc = "            \"turn_fail\""]
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
pub struct BrainAgentloopContract {
    pub contract: BrainAgentloopContractContract,
}
#[doc = "`BrainAgentloopContractContract`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"activations\","]
#[doc = "    \"ops\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"activations\": {"]
#[doc = "      \"const\": ["]
#[doc = "        \"session_start\","]
#[doc = "        \"message\","]
#[doc = "        \"session_end\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"ops\": {"]
#[doc = "      \"const\": ["]
#[doc = "        \"model_stream\","]
#[doc = "        \"tools_dispatch\","]
#[doc = "        \"journal_append\","]
#[doc = "        \"kv_get\","]
#[doc = "        \"kv_set\","]
#[doc = "        \"journal_read\","]
#[doc = "        \"turn_finish\","]
#[doc = "        \"turn_fail\""]
#[doc = "      ]"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct BrainAgentloopContractContract {
    pub activations: ::serde_json::Value,
    pub ops: ::serde_json::Value,
}
#[doc = "One content block. v1 is text and tool calls; richer media rides object references in a later additive revision."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"One content block. v1 is text and tool calls; richer media rides object references in a later additive revision.\","]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"$ref\": \"#/$defs/TextContentView\""]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"$ref\": \"#/$defs/ToolCallContentView\""]
#[doc = "    }"]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(untagged)]
pub enum ContentView {
    TextContentView(TextContentView),
    ToolCallContentView(ToolCallContentView),
}
impl ::std::convert::From<TextContentView> for ContentView {
    fn from(value: TextContentView) -> Self {
        Self::TextContentView(value)
    }
}
impl ::std::convert::From<ToolCallContentView> for ContentView {
    fn from(value: ToolCallContentView) -> Self {
        Self::ToolCallContentView(value)
    }
}
#[doc = "`CtxOp`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"op\","]
#[doc = "        \"request\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"model_stream\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"request\": {"]
#[doc = "          \"$ref\": \"#/$defs/ModelRequest\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"calls\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"calls\": {"]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"$ref\": \"#/$defs/ToolCallRequest\""]
#[doc = "          },"]
#[doc = "          \"maxItems\": 8,"]
#[doc = "          \"minItems\": 1"]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"tools_dispatch\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"entries\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"entries\": {"]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"$ref\": \"#/$defs/LoopEntry\""]
#[doc = "          },"]
#[doc = "          \"maxItems\": 64,"]
#[doc = "          \"minItems\": 1"]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"journal_append\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"keys\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"keys\": {"]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"type\": \"string\","]
#[doc = "            \"maxLength\": 128,"]
#[doc = "            \"minLength\": 1"]
#[doc = "          },"]
#[doc = "          \"maxItems\": 64,"]
#[doc = "          \"minItems\": 1"]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"kv_get\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"entries\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"entries\": {"]
#[doc = "          \"description\": \"Key to JSON value; null deletes the key. Last writer wins per key.\","]
#[doc = "          \"$ref\": \"#/$defs/JsonObject\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"kv_set\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"after_seq\": {"]
#[doc = "          \"$ref\": \"#/$defs/Seq\""]
#[doc = "        },"]
#[doc = "        \"limit\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"maximum\": 256.0,"]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"journal_read\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"types\": {"]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"type\": \"string\","]
#[doc = "            \"enum\": ["]
#[doc = "              \"user_message\","]
#[doc = "              \"assistant_message\","]
#[doc = "              \"tool_result\","]
#[doc = "              \"loop_custom\","]
#[doc = "              \"loop_event\","]
#[doc = "              \"loop_mark\""]
#[doc = "            ]"]
#[doc = "          },"]
#[doc = "          \"maxItems\": 6,"]
#[doc = "          \"minItems\": 1"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"turn_finish\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"result\": {"]
#[doc = "          \"$ref\": \"#/$defs/JsonObject\""]
#[doc = "        },"]
#[doc = "        \"stop_reason\": {"]
#[doc = "          \"description\": \"The loop's terminal claim for this turn; absent means end_turn. Cancelled and interrupted stay kernel-owned outcomes a loop cannot claim.\","]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"end_turn\","]
#[doc = "            \"max_rounds\","]
#[doc = "            \"refusal\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"error\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"error\": {"]
#[doc = "          \"$ref\": \"#/$defs/AgentloopError\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"turn_fail\""]
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
#[serde(tag = "op", deny_unknown_fields)]
pub enum CtxOp {
    #[serde(rename = "model_stream")]
    ModelStream { request: ModelRequest },
    #[serde(rename = "tools_dispatch")]
    ToolsDispatch {
        calls: ::std::vec::Vec<ToolCallRequest>,
    },
    #[serde(rename = "journal_append")]
    JournalAppend { entries: ::std::vec::Vec<LoopEntry> },
    #[serde(rename = "kv_get")]
    KvGet {
        keys: ::std::vec::Vec<CtxOpKeysItem>,
    },
    #[serde(rename = "kv_set")]
    KvSet {
        #[doc = "Key to JSON value; null deletes the key. Last writer wins per key."]
        entries: JsonObject,
    },
    #[serde(rename = "journal_read")]
    JournalRead {
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        after_seq: ::std::option::Option<Seq>,
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        limit: ::std::option::Option<::std::num::NonZeroU64>,
        #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
        types: ::std::vec::Vec<CtxOpTypesItem>,
    },
    #[serde(rename = "turn_finish")]
    TurnFinish {
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        result: ::std::option::Option<JsonObject>,
        #[doc = "The loop's terminal claim for this turn; absent means end_turn. Cancelled and interrupted stay kernel-owned outcomes a loop cannot claim."]
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        stop_reason: ::std::option::Option<CtxOpStopReason>,
    },
    #[serde(rename = "turn_fail")]
    TurnFail { error: AgentloopError },
}
#[doc = "`CtxOpKeysItem`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 128,"]
#[doc = "  \"minLength\": 1"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct CtxOpKeysItem(::std::string::String);
impl ::std::ops::Deref for CtxOpKeysItem {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<CtxOpKeysItem> for ::std::string::String {
    fn from(value: CtxOpKeysItem) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for CtxOpKeysItem {
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
impl ::std::convert::TryFrom<&str> for CtxOpKeysItem {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CtxOpKeysItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CtxOpKeysItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for CtxOpKeysItem {
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
#[doc = "One loop-to-Brain capability call. Journaled before its effect where effectful; deduplicated by (op_id, canonical request digest)."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"One loop-to-Brain capability call. Journaled before its effect where effectful; deduplicated by (op_id, canonical request digest).\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"activation_id\","]
#[doc = "    \"op\","]
#[doc = "    \"op_id\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"activation_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/Identifier\""]
#[doc = "    },"]
#[doc = "    \"op\": {"]
#[doc = "      \"$ref\": \"#/$defs/CtxOp\""]
#[doc = "    },"]
#[doc = "    \"op_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/Identifier\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct CtxOpRequest {
    pub activation_id: Identifier,
    pub op: CtxOp,
    pub op_id: Identifier,
}
#[doc = "Exactly one of result or error."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Exactly one of result or error.\","]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"op_id\","]
#[doc = "        \"result\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"op_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/Identifier\""]
#[doc = "        },"]
#[doc = "        \"result\": {"]
#[doc = "          \"$ref\": \"#/$defs/CtxOpResult\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"error\","]
#[doc = "        \"op_id\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"error\": {"]
#[doc = "          \"$ref\": \"#/$defs/AgentloopError\""]
#[doc = "        },"]
#[doc = "        \"op_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/Identifier\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    }"]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(untagged, deny_unknown_fields)]
pub enum CtxOpResponse {
    Variant0 {
        op_id: Identifier,
        result: CtxOpResult,
    },
    Variant1 {
        error: AgentloopError,
        op_id: Identifier,
    },
}
#[doc = "`CtxOpResult`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"message\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"message\": {"]
#[doc = "          \"$ref\": \"#/$defs/AssistantMessageView\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"model_stream\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"op\","]
#[doc = "        \"results\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"tools_dispatch\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"results\": {"]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"$ref\": \"#/$defs/ToolResultView\""]
#[doc = "          },"]
#[doc = "          \"maxItems\": 8,"]
#[doc = "          \"minItems\": 1"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"first_seq\","]
#[doc = "        \"last_seq\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"first_seq\": {"]
#[doc = "          \"$ref\": \"#/$defs/Seq\""]
#[doc = "        },"]
#[doc = "        \"last_seq\": {"]
#[doc = "          \"$ref\": \"#/$defs/Seq\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"journal_append\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"entries\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"entries\": {"]
#[doc = "          \"$ref\": \"#/$defs/JsonObject\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"kv_get\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"kv_set\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"entries\","]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"entries\": {"]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"$ref\": \"#/$defs/JournalEntryView\""]
#[doc = "          },"]
#[doc = "          \"maxItems\": 256"]
#[doc = "        },"]
#[doc = "        \"next_after_seq\": {"]
#[doc = "          \"$ref\": \"#/$defs/Seq\""]
#[doc = "        },"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"journal_read\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"turn_finish\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"op\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"op\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"turn_fail\""]
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
#[serde(tag = "op", deny_unknown_fields)]
pub enum CtxOpResult {
    #[serde(rename = "model_stream")]
    ModelStream { message: AssistantMessageView },
    #[serde(rename = "tools_dispatch")]
    ToolsDispatch {
        results: ::std::vec::Vec<ToolResultView>,
    },
    #[serde(rename = "journal_append")]
    JournalAppend { first_seq: Seq, last_seq: Seq },
    #[serde(rename = "kv_get")]
    KvGet { entries: JsonObject },
    #[serde(rename = "kv_set")]
    KvSet,
    #[serde(rename = "journal_read")]
    JournalRead {
        entries: ::std::vec::Vec<JournalEntryView>,
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        next_after_seq: ::std::option::Option<Seq>,
    },
    #[serde(rename = "turn_finish")]
    TurnFinish,
    #[serde(rename = "turn_fail")]
    TurnFail,
}
#[doc = "The loop's terminal claim for this turn; absent means end_turn. Cancelled and interrupted stay kernel-owned outcomes a loop cannot claim."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"The loop's terminal claim for this turn; absent means end_turn. Cancelled and interrupted stay kernel-owned outcomes a loop cannot claim.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"end_turn\","]
#[doc = "    \"max_rounds\","]
#[doc = "    \"refusal\""]
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
pub enum CtxOpStopReason {
    #[serde(rename = "end_turn")]
    EndTurn,
    #[serde(rename = "max_rounds")]
    MaxRounds,
    #[serde(rename = "refusal")]
    Refusal,
}
impl ::std::fmt::Display for CtxOpStopReason {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::EndTurn => f.write_str("end_turn"),
            Self::MaxRounds => f.write_str("max_rounds"),
            Self::Refusal => f.write_str("refusal"),
        }
    }
}
impl ::std::str::FromStr for CtxOpStopReason {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "end_turn" => Ok(Self::EndTurn),
            "max_rounds" => Ok(Self::MaxRounds),
            "refusal" => Ok(Self::Refusal),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for CtxOpStopReason {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CtxOpStopReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CtxOpStopReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`CtxOpTypesItem`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"user_message\","]
#[doc = "    \"assistant_message\","]
#[doc = "    \"tool_result\","]
#[doc = "    \"loop_custom\","]
#[doc = "    \"loop_event\","]
#[doc = "    \"loop_mark\""]
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
pub enum CtxOpTypesItem {
    #[serde(rename = "user_message")]
    UserMessage,
    #[serde(rename = "assistant_message")]
    AssistantMessage,
    #[serde(rename = "tool_result")]
    ToolResult,
    #[serde(rename = "loop_custom")]
    LoopCustom,
    #[serde(rename = "loop_event")]
    LoopEvent,
    #[serde(rename = "loop_mark")]
    LoopMark,
}
impl ::std::fmt::Display for CtxOpTypesItem {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::UserMessage => f.write_str("user_message"),
            Self::AssistantMessage => f.write_str("assistant_message"),
            Self::ToolResult => f.write_str("tool_result"),
            Self::LoopCustom => f.write_str("loop_custom"),
            Self::LoopEvent => f.write_str("loop_event"),
            Self::LoopMark => f.write_str("loop_mark"),
        }
    }
}
impl ::std::str::FromStr for CtxOpTypesItem {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "user_message" => Ok(Self::UserMessage),
            "assistant_message" => Ok(Self::AssistantMessage),
            "tool_result" => Ok(Self::ToolResult),
            "loop_custom" => Ok(Self::LoopCustom),
            "loop_event" => Ok(Self::LoopEvent),
            "loop_mark" => Ok(Self::LoopMark),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for CtxOpTypesItem {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for CtxOpTypesItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CtxOpTypesItem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
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
#[doc = "A versioned typed projection of one journal entry. Kernel records project to these stable views; the raw record encoding stays internal."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"A versioned typed projection of one journal entry. Kernel records project to these stable views; the raw record encoding stays internal.\","]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"at\","]
#[doc = "        \"content\","]
#[doc = "        \"seq\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"content\": {"]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"$ref\": \"#/$defs/ContentView\""]
#[doc = "          },"]
#[doc = "          \"maxItems\": 64"]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"$ref\": \"#/$defs/Seq\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"user_message\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"at\","]
#[doc = "        \"message\","]
#[doc = "        \"seq\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"message\": {"]
#[doc = "          \"$ref\": \"#/$defs/AssistantMessageView\""]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"$ref\": \"#/$defs/Seq\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"assistant_message\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"at\","]
#[doc = "        \"result\","]
#[doc = "        \"seq\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"result\": {"]
#[doc = "          \"$ref\": \"#/$defs/ToolResultView\""]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"$ref\": \"#/$defs/Seq\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"tool_result\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"at\","]
#[doc = "        \"data\","]
#[doc = "        \"seq\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"data\": {"]
#[doc = "          \"$ref\": \"#/$defs/JsonObject\""]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"$ref\": \"#/$defs/Seq\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"loop_custom\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"at\","]
#[doc = "        \"data\","]
#[doc = "        \"name\","]
#[doc = "        \"seq\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"data\": {"]
#[doc = "          \"$ref\": \"#/$defs/JsonObject\""]
#[doc = "        },"]
#[doc = "        \"name\": {"]
#[doc = "          \"$ref\": \"#/$defs/Identifier\""]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"$ref\": \"#/$defs/Seq\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"loop_event\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"at\","]
#[doc = "        \"covers_through_seq\","]
#[doc = "        \"data\","]
#[doc = "        \"seq\","]
#[doc = "        \"type\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"at\": {"]
#[doc = "          \"$ref\": \"#/$defs/Timestamp\""]
#[doc = "        },"]
#[doc = "        \"covers_through_seq\": {"]
#[doc = "          \"$ref\": \"#/$defs/Seq\""]
#[doc = "        },"]
#[doc = "        \"data\": {"]
#[doc = "          \"$ref\": \"#/$defs/JsonObject\""]
#[doc = "        },"]
#[doc = "        \"seq\": {"]
#[doc = "          \"$ref\": \"#/$defs/Seq\""]
#[doc = "        },"]
#[doc = "        \"type\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"loop_mark\""]
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
pub enum JournalEntryView {
    #[serde(rename = "user_message")]
    UserMessage {
        at: Timestamp,
        content: ::std::vec::Vec<ContentView>,
        seq: Seq,
    },
    #[serde(rename = "assistant_message")]
    AssistantMessage {
        at: Timestamp,
        message: AssistantMessageView,
        seq: Seq,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        at: Timestamp,
        result: ToolResultView,
        seq: Seq,
    },
    #[serde(rename = "loop_custom")]
    LoopCustom {
        at: Timestamp,
        data: JsonObject,
        seq: Seq,
    },
    #[serde(rename = "loop_event")]
    LoopEvent {
        at: Timestamp,
        data: JsonObject,
        name: Identifier,
        seq: Seq,
    },
    #[serde(rename = "loop_mark")]
    LoopMark {
        at: Timestamp,
        covers_through_seq: Seq,
        data: JsonObject,
        seq: Seq,
    },
}
#[doc = "An opaque bounded JSON object. Byte bounds are enforced at the execution boundary, not expressible in schema."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"An opaque bounded JSON object. Byte bounds are enforced at the execution boundary, not expressible in schema.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"additionalProperties\": true"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(transparent)]
pub struct JsonObject(pub ::serde_json::Map<::std::string::String, ::serde_json::Value>);
impl ::std::ops::Deref for JsonObject {
    type Target = ::serde_json::Map<::std::string::String, ::serde_json::Value>;
    fn deref(&self) -> &::serde_json::Map<::std::string::String, ::serde_json::Value> {
        &self.0
    }
}
impl ::std::convert::From<JsonObject>
    for ::serde_json::Map<::std::string::String, ::serde_json::Value>
{
    fn from(value: JsonObject) -> Self {
        value.0
    }
}
impl ::std::convert::From<::serde_json::Map<::std::string::String, ::serde_json::Value>>
    for JsonObject
{
    fn from(value: ::serde_json::Map<::std::string::String, ::serde_json::Value>) -> Self {
        Self(value)
    }
}
#[doc = "One loop-authored durable journal entry. Entries commit with the next kernel decision (or activation end, whichever comes first) and count against the tenant journal quota."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"One loop-authored durable journal entry. Entries commit with the next kernel decision (or activation end, whichever comes first) and count against the tenant journal quota.\","]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"data\","]
#[doc = "        \"kind\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"data\": {"]
#[doc = "          \"$ref\": \"#/$defs/JsonObject\""]
#[doc = "        },"]
#[doc = "        \"kind\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"custom\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"description\": \"Application-visible: surfaces on the session event stream as loop.event.\","]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"data\","]
#[doc = "        \"kind\","]
#[doc = "        \"name\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"data\": {"]
#[doc = "          \"$ref\": \"#/$defs/JsonObject\""]
#[doc = "        },"]
#[doc = "        \"kind\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"event\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"name\": {"]
#[doc = "          \"$ref\": \"#/$defs/Identifier\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"description\": \"The hydration floor: data carries the loop's compacted working context (chunked internally when large); the next session_start tail begins after covers_through_seq.\","]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"covers_through_seq\","]
#[doc = "        \"data\","]
#[doc = "        \"kind\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"covers_through_seq\": {"]
#[doc = "          \"$ref\": \"#/$defs/Seq\""]
#[doc = "        },"]
#[doc = "        \"data\": {"]
#[doc = "          \"$ref\": \"#/$defs/JsonObject\""]
#[doc = "        },"]
#[doc = "        \"kind\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"mark\""]
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
#[serde(tag = "kind", deny_unknown_fields)]
pub enum LoopEntry {
    #[serde(rename = "custom")]
    Custom { data: JsonObject },
    #[doc = "Application-visible: surfaces on the session event stream as loop.event."]
    #[serde(rename = "event")]
    Event { data: JsonObject, name: Identifier },
    #[doc = "The hydration floor: data carries the loop's compacted working context (chunked internally when large); the next session_start tail begins after covers_through_seq."]
    #[serde(rename = "mark")]
    Mark {
        covers_through_seq: Seq,
        data: JsonObject,
    },
}
#[doc = "`MarkView`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"covers_through_seq\","]
#[doc = "    \"data\","]
#[doc = "    \"seq\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"covers_through_seq\": {"]
#[doc = "      \"$ref\": \"#/$defs/Seq\""]
#[doc = "    },"]
#[doc = "    \"data\": {"]
#[doc = "      \"$ref\": \"#/$defs/JsonObject\""]
#[doc = "    },"]
#[doc = "    \"seq\": {"]
#[doc = "      \"$ref\": \"#/$defs/Seq\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct MarkView {
    pub covers_through_seq: Seq,
    pub data: JsonObject,
    pub seq: Seq,
}
#[doc = "One provider-visible message the loop composes. Presentation is loop policy; authority is not: Brain validates every named tool against the sealed grant."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"One provider-visible message the loop composes. Presentation is loop policy; authority is not: Brain validates every named tool against the sealed grant.\","]
#[doc = "  \"oneOf\": ["]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"content\","]
#[doc = "        \"role\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"content\": {"]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"$ref\": \"#/$defs/ContentView\""]
#[doc = "          },"]
#[doc = "          \"maxItems\": 64"]
#[doc = "        },"]
#[doc = "        \"role\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"user\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"content\","]
#[doc = "        \"role\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"content\": {"]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"$ref\": \"#/$defs/ContentView\""]
#[doc = "          },"]
#[doc = "          \"maxItems\": 64"]
#[doc = "        },"]
#[doc = "        \"role\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"assistant\""]
#[doc = "          ]"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    {"]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"content\","]
#[doc = "        \"name\","]
#[doc = "        \"role\","]
#[doc = "        \"tool_call_id\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"content\": {"]
#[doc = "          \"type\": \"array\","]
#[doc = "          \"items\": {"]
#[doc = "            \"$ref\": \"#/$defs/TextContentView\""]
#[doc = "          },"]
#[doc = "          \"maxItems\": 64"]
#[doc = "        },"]
#[doc = "        \"is_error\": {"]
#[doc = "          \"type\": \"boolean\""]
#[doc = "        },"]
#[doc = "        \"name\": {"]
#[doc = "          \"$ref\": \"#/$defs/Identifier\""]
#[doc = "        },"]
#[doc = "        \"role\": {"]
#[doc = "          \"type\": \"string\","]
#[doc = "          \"enum\": ["]
#[doc = "            \"tool_result\""]
#[doc = "          ]"]
#[doc = "        },"]
#[doc = "        \"tool_call_id\": {"]
#[doc = "          \"$ref\": \"#/$defs/Identifier\""]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    }"]
#[doc = "  ]"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(tag = "role", deny_unknown_fields)]
pub enum ModelMessage {
    #[serde(rename = "user")]
    User {
        content: ::std::vec::Vec<ContentView>,
    },
    #[serde(rename = "assistant")]
    Assistant {
        content: ::std::vec::Vec<ContentView>,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        content: ::std::vec::Vec<TextContentView>,
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        is_error: ::std::option::Option<bool>,
        name: Identifier,
        tool_call_id: Identifier,
    },
}
#[doc = "A provider model identifier as the session sealed it. Unlike Identifier this admits gateway-style names with path separators (e.g. \"openai/gpt-4.1-nano\")."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"A provider model identifier as the session sealed it. Unlike Identifier this admits gateway-style names with path separators (e.g. \\\"openai/gpt-4.1-nano\\\").\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 256,"]
#[doc = "  \"minLength\": 1,"]
#[doc = "  \"pattern\": \"^[A-Za-z0-9][A-Za-z0-9._:/-]{0,255}$\""]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ModelName(::std::string::String);
impl ::std::ops::Deref for ModelName {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ModelName> for ::std::string::String {
    fn from(value: ModelName) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ModelName {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 256usize {
            return Err("longer than 256 characters".into());
        }
        if value.chars().count() < 1usize {
            return Err("shorter than 1 characters".into());
        }
        static PATTERN: ::std::sync::LazyLock<::regress::Regex> =
            ::std::sync::LazyLock::new(|| {
                ::regress::Regex::new("^[A-Za-z0-9][A-Za-z0-9._:/-]{0,255}$").unwrap()
            });
        if PATTERN.find(value).is_none() {
            return Err("doesn't match pattern \"^[A-Za-z0-9][A-Za-z0-9._:/-]{0,255}$\"".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ModelName {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ModelName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ModelName {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ModelName {
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
#[doc = "A composed provider request. Brain executes it against the session's sealed provider and model with custody, live retry and attempt recovery, and journals intent and result. The loop never selects a provider, model or credential."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"A composed provider request. Brain executes it against the session's sealed provider and model with custody, live retry and attempt recovery, and journals intent and result. The loop never selects a provider, model or credential.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"messages\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"max_tokens\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"messages\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/ModelMessage\""]
#[doc = "      },"]
#[doc = "      \"maxItems\": 4096,"]
#[doc = "      \"minItems\": 1"]
#[doc = "    },"]
#[doc = "    \"system\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 131072"]
#[doc = "    },"]
#[doc = "    \"temperature\": {"]
#[doc = "      \"type\": \"number\","]
#[doc = "      \"maximum\": 2.0,"]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"tool_choice\": {"]
#[doc = "      \"description\": \"Only the closing-round constraint: presented tools stay on the wire (tool-block histories require them) while the model is asked to answer in text. Absent means the provider default.\","]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"none\""]
#[doc = "      ]"]
#[doc = "    },"]
#[doc = "    \"tools\": {"]
#[doc = "      \"description\": \"Absent or empty means the sealed presentation verbatim, which also keeps the provider's frozen base segment and prompt-cache key. A non-empty list re-presents sealed tools by name and pays its own cache economics.\","]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/ToolPresentationView\""]
#[doc = "      },"]
#[doc = "      \"maxItems\": 128"]
#[doc = "    },"]
#[doc = "    \"top_p\": {"]
#[doc = "      \"type\": \"number\","]
#[doc = "      \"maximum\": 1.0,"]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ModelRequest {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub max_tokens: ::std::option::Option<::std::num::NonZeroU64>,
    pub messages: ::std::vec::Vec<ModelMessage>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub system: ::std::option::Option<ModelRequestSystem>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub temperature: ::std::option::Option<f64>,
    #[doc = "Only the closing-round constraint: presented tools stay on the wire (tool-block histories require them) while the model is asked to answer in text. Absent means the provider default."]
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub tool_choice: ::std::option::Option<ModelRequestToolChoice>,
    #[doc = "Absent or empty means the sealed presentation verbatim, which also keeps the provider's frozen base segment and prompt-cache key. A non-empty list re-presents sealed tools by name and pays its own cache economics."]
    #[serde(default, skip_serializing_if = "::std::vec::Vec::is_empty")]
    pub tools: ::std::vec::Vec<ToolPresentationView>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub top_p: ::std::option::Option<f64>,
}
#[doc = "`ModelRequestSystem`"]
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
pub struct ModelRequestSystem(::std::string::String);
impl ::std::ops::Deref for ModelRequestSystem {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ModelRequestSystem> for ::std::string::String {
    fn from(value: ModelRequestSystem) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ModelRequestSystem {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 131072usize {
            return Err("longer than 131072 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ModelRequestSystem {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ModelRequestSystem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ModelRequestSystem {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ModelRequestSystem {
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
#[doc = "Only the closing-round constraint: presented tools stay on the wire (tool-block histories require them) while the model is asked to answer in text. Absent means the provider default."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Only the closing-round constraint: presented tools stay on the wire (tool-block histories require them) while the model is asked to answer in text. Absent means the provider default.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"none\""]
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
pub enum ModelRequestToolChoice {
    #[serde(rename = "none")]
    None,
}
impl ::std::fmt::Display for ModelRequestToolChoice {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::None => f.write_str("none"),
        }
    }
}
impl ::std::str::FromStr for ModelRequestToolChoice {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "none" => Ok(Self::None),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ModelRequestToolChoice {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ModelRequestToolChoice {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ModelRequestToolChoice {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "Why one model round stopped."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Why one model round stopped.\","]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"end_turn\","]
#[doc = "    \"tool_use\","]
#[doc = "    \"max_tokens\","]
#[doc = "    \"refusal\""]
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
pub enum ModelStopReason {
    #[serde(rename = "end_turn")]
    EndTurn,
    #[serde(rename = "tool_use")]
    ToolUse,
    #[serde(rename = "max_tokens")]
    MaxTokens,
    #[serde(rename = "refusal")]
    Refusal,
}
impl ::std::fmt::Display for ModelStopReason {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::EndTurn => f.write_str("end_turn"),
            Self::ToolUse => f.write_str("tool_use"),
            Self::MaxTokens => f.write_str("max_tokens"),
            Self::Refusal => f.write_str("refusal"),
        }
    }
}
impl ::std::str::FromStr for ModelStopReason {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "end_turn" => Ok(Self::EndTurn),
            "tool_use" => Ok(Self::ToolUse),
            "max_tokens" => Ok(Self::MaxTokens),
            "refusal" => Ok(Self::Refusal),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ModelStopReason {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ModelStopReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ModelStopReason {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "A journal sequence number of this session."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"A journal sequence number of this session.\","]
#[doc = "  \"type\": \"integer\","]
#[doc = "  \"minimum\": 1.0"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(transparent)]
pub struct Seq(pub ::std::num::NonZeroU64);
impl ::std::ops::Deref for Seq {
    type Target = ::std::num::NonZeroU64;
    fn deref(&self) -> &::std::num::NonZeroU64 {
        &self.0
    }
}
impl ::std::convert::From<Seq> for ::std::num::NonZeroU64 {
    fn from(value: Seq) -> Self {
        value.0
    }
}
impl ::std::convert::From<::std::num::NonZeroU64> for Seq {
    fn from(value: ::std::num::NonZeroU64) -> Self {
        Self(value)
    }
}
impl ::std::str::FromStr for Seq {
    type Err = <::std::num::NonZeroU64 as ::std::str::FromStr>::Err;
    fn from_str(value: &str) -> ::std::result::Result<Self, Self::Err> {
        Ok(Self(value.parse()?))
    }
}
impl ::std::convert::TryFrom<&str> for Seq {
    type Error = <::std::num::NonZeroU64 as ::std::str::FromStr>::Err;
    fn try_from(value: &str) -> ::std::result::Result<Self, Self::Error> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<String> for Seq {
    type Error = <::std::num::NonZeroU64 as ::std::str::FromStr>::Err;
    fn try_from(value: String) -> ::std::result::Result<Self, Self::Error> {
        value.parse()
    }
}
impl ::std::fmt::Display for Seq {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        self.0.fmt(f)
    }
}
#[doc = "`SessionContextView`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"limits\","]
#[doc = "    \"model\","]
#[doc = "    \"session_id\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"limits\": {"]
#[doc = "      \"description\": \"Kernel-enforced authorization, not advisory policy. The kernel rejects work past these regardless of loop behavior.\","]
#[doc = "      \"type\": \"object\","]
#[doc = "      \"required\": ["]
#[doc = "        \"max_parallel_tools\","]
#[doc = "        \"max_rounds_per_turn\","]
#[doc = "        \"turn_wall_ms\""]
#[doc = "      ],"]
#[doc = "      \"properties\": {"]
#[doc = "        \"max_parallel_tools\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"max_rounds_per_turn\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        },"]
#[doc = "        \"turn_wall_ms\": {"]
#[doc = "          \"type\": \"integer\","]
#[doc = "          \"minimum\": 1.0"]
#[doc = "        }"]
#[doc = "      },"]
#[doc = "      \"additionalProperties\": false"]
#[doc = "    },"]
#[doc = "    \"metadata\": {"]
#[doc = "      \"$ref\": \"#/$defs/JsonObject\""]
#[doc = "    },"]
#[doc = "    \"model\": {"]
#[doc = "      \"$ref\": \"#/$defs/ModelName\""]
#[doc = "    },"]
#[doc = "    \"session_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/SessionId\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct SessionContextView {
    pub limits: SessionContextViewLimits,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub metadata: ::std::option::Option<JsonObject>,
    pub model: ModelName,
    pub session_id: SessionId,
}
#[doc = "Kernel-enforced authorization, not advisory policy. The kernel rejects work past these regardless of loop behavior."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Kernel-enforced authorization, not advisory policy. The kernel rejects work past these regardless of loop behavior.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"max_parallel_tools\","]
#[doc = "    \"max_rounds_per_turn\","]
#[doc = "    \"turn_wall_ms\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"max_parallel_tools\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"max_rounds_per_turn\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 1.0"]
#[doc = "    },"]
#[doc = "    \"turn_wall_ms\": {"]
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
pub struct SessionContextViewLimits {
    pub max_parallel_tools: ::std::num::NonZeroU64,
    pub max_rounds_per_turn: ::std::num::NonZeroU64,
    pub turn_wall_ms: ::std::num::NonZeroU64,
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
#[doc = "`TextContentView`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"text\","]
#[doc = "    \"type\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"text\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 196608"]
#[doc = "    },"]
#[doc = "    \"type\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"text\""]
#[doc = "      ]"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct TextContentView {
    pub text: TextContentViewText,
    #[serde(rename = "type")]
    pub type_: TextContentViewType,
}
#[doc = "`TextContentViewText`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 196608"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct TextContentViewText(::std::string::String);
impl ::std::ops::Deref for TextContentViewText {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<TextContentViewText> for ::std::string::String {
    fn from(value: TextContentViewText) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for TextContentViewText {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 196608usize {
            return Err("longer than 196608 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for TextContentViewText {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for TextContentViewText {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for TextContentViewText {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for TextContentViewText {
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
#[doc = "`TextContentViewType`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"text\""]
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
pub enum TextContentViewType {
    #[serde(rename = "text")]
    Text,
}
impl ::std::fmt::Display for TextContentViewType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Text => f.write_str("text"),
        }
    }
}
impl ::std::str::FromStr for TextContentViewType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "text" => Ok(Self::Text),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for TextContentViewType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for TextContentViewType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for TextContentViewType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`Timestamp`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"format\": \"date-time\","]
#[doc = "  \"maxLength\": 40"]
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
#[doc = "`ToolCallContentView`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"input\","]
#[doc = "    \"name\","]
#[doc = "    \"tool_call_id\","]
#[doc = "    \"type\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"input\": {"]
#[doc = "      \"$ref\": \"#/$defs/JsonObject\""]
#[doc = "    },"]
#[doc = "    \"name\": {"]
#[doc = "      \"$ref\": \"#/$defs/Identifier\""]
#[doc = "    },"]
#[doc = "    \"tool_call_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/Identifier\""]
#[doc = "    },"]
#[doc = "    \"type\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"enum\": ["]
#[doc = "        \"tool_call\""]
#[doc = "      ]"]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ToolCallContentView {
    pub input: JsonObject,
    pub name: Identifier,
    pub tool_call_id: Identifier,
    #[serde(rename = "type")]
    pub type_: ToolCallContentViewType,
}
#[doc = "`ToolCallContentViewType`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"enum\": ["]
#[doc = "    \"tool_call\""]
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
pub enum ToolCallContentViewType {
    #[serde(rename = "tool_call")]
    ToolCall,
}
impl ::std::fmt::Display for ToolCallContentViewType {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::ToolCall => f.write_str("tool_call"),
        }
    }
}
impl ::std::str::FromStr for ToolCallContentViewType {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "tool_call" => Ok(Self::ToolCall),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ToolCallContentViewType {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolCallContentViewType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolCallContentViewType {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
#[doc = "`ToolCallRequest`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"input\","]
#[doc = "    \"name\","]
#[doc = "    \"tool_call_id\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"input\": {"]
#[doc = "      \"$ref\": \"#/$defs/JsonObject\""]
#[doc = "    },"]
#[doc = "    \"name\": {"]
#[doc = "      \"$ref\": \"#/$defs/Identifier\""]
#[doc = "    },"]
#[doc = "    \"tool_call_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/Identifier\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ToolCallRequest {
    pub input: JsonObject,
    pub name: Identifier,
    pub tool_call_id: Identifier,
}
#[doc = "How the loop presents one sealed tool to the model on one request. Showing a subset, reordering, or rewording is loop policy; the executable binding stays sealed."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"How the loop presents one sealed tool to the model on one request. Showing a subset, reordering, or rewording is loop policy; the executable binding stays sealed.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"input_schema\","]
#[doc = "    \"name\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"description\": {"]
#[doc = "      \"type\": \"string\","]
#[doc = "      \"maxLength\": 8192"]
#[doc = "    },"]
#[doc = "    \"input_schema\": {"]
#[doc = "      \"$ref\": \"#/$defs/JsonObject\""]
#[doc = "    },"]
#[doc = "    \"name\": {"]
#[doc = "      \"description\": \"Must name a tool in the session's sealed grant. An unsealed name fails the request; it is never routed.\","]
#[doc = "      \"$ref\": \"#/$defs/Identifier\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ToolPresentationView {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub description: ::std::option::Option<ToolPresentationViewDescription>,
    pub input_schema: JsonObject,
    #[doc = "Must name a tool in the session's sealed grant. An unsealed name fails the request; it is never routed."]
    pub name: Identifier,
}
#[doc = "`ToolPresentationViewDescription`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"string\","]
#[doc = "  \"maxLength\": 8192"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct ToolPresentationViewDescription(::std::string::String);
impl ::std::ops::Deref for ToolPresentationViewDescription {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<ToolPresentationViewDescription> for ::std::string::String {
    fn from(value: ToolPresentationViewDescription) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for ToolPresentationViewDescription {
    type Err = self::error::ConversionError;
    fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 8192usize {
            return Err("longer than 8192 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for ToolPresentationViewDescription {
    type Error = self::error::ConversionError;
    fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<&::std::string::String> for ToolPresentationViewDescription {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ToolPresentationViewDescription {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for ToolPresentationViewDescription {
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
#[doc = "`ToolResultView`"]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"required\": ["]
#[doc = "    \"content\","]
#[doc = "    \"is_error\","]
#[doc = "    \"name\","]
#[doc = "    \"tool_call_id\""]
#[doc = "  ],"]
#[doc = "  \"properties\": {"]
#[doc = "    \"content\": {"]
#[doc = "      \"type\": \"array\","]
#[doc = "      \"items\": {"]
#[doc = "        \"$ref\": \"#/$defs/TextContentView\""]
#[doc = "      },"]
#[doc = "      \"maxItems\": 64"]
#[doc = "    },"]
#[doc = "    \"is_error\": {"]
#[doc = "      \"type\": \"boolean\""]
#[doc = "    },"]
#[doc = "    \"name\": {"]
#[doc = "      \"$ref\": \"#/$defs/Identifier\""]
#[doc = "    },"]
#[doc = "    \"tool_call_id\": {"]
#[doc = "      \"$ref\": \"#/$defs/Identifier\""]
#[doc = "    }"]
#[doc = "  },"]
#[doc = "  \"additionalProperties\": false"]
#[doc = "}"]
#[doc = r" ```"]
#[doc = r" </details>"]
#[derive(:: serde :: Deserialize, :: serde :: Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ToolResultView {
    pub content: ::std::vec::Vec<TextContentView>,
    pub is_error: bool,
    pub name: Identifier,
    pub tool_call_id: Identifier,
}
#[doc = "Provider-reported usage. Absent counters are absent, never zero."]
#[doc = r""]
#[doc = r" <details><summary>JSON schema</summary>"]
#[doc = r""]
#[doc = r" ```json"]
#[doc = "{"]
#[doc = "  \"description\": \"Provider-reported usage. Absent counters are absent, never zero.\","]
#[doc = "  \"type\": \"object\","]
#[doc = "  \"properties\": {"]
#[doc = "    \"cache_read_tokens\": {"]
#[doc = "      \"type\": \"integer\","]
#[doc = "      \"minimum\": 0.0"]
#[doc = "    },"]
#[doc = "    \"cache_write_tokens\": {"]
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
#[doc = "    \"total_tokens\": {"]
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
pub struct UsageView {
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub cache_read_tokens: ::std::option::Option<u64>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub cache_write_tokens: ::std::option::Option<u64>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub input_tokens: ::std::option::Option<u64>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub output_tokens: ::std::option::Option<u64>,
    #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
    pub total_tokens: ::std::option::Option<u64>,
}
impl ::std::default::Default for UsageView {
    fn default() -> Self {
        Self {
            cache_read_tokens: Default::default(),
            cache_write_tokens: Default::default(),
            input_tokens: Default::default(),
            output_tokens: Default::default(),
            total_tokens: Default::default(),
        }
    }
}
