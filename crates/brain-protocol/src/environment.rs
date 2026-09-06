use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{AgentloopId, EnvironmentName, HostId, Outcome, SessionId, TurnInput, TurnOutput};

/// The contract identifier every command and response carries.
pub const ENVIRONMENT_CONTRACT: &str = "environment/v1";

/// How Brain reaches an Environment. Applications never write it: the SDK does, from
/// the Environment the application chose. Where the code behind the protocol runs is
/// the Environment's concern.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "driver", rename_all = "snake_case", deny_unknown_fields)]
pub enum Driver {
    /// Hosted inside brain-server.
    Brain {},
    /// The process that registered as this host, reached over the connection it holds
    /// open: a browser tab, a Node process, a server.
    Host { host_id: HostId },
    /// Reached over HTTP. `credential` is sent as a bearer token; the server seals it
    /// beside the model key and never journals it.
    Http {
        #[schemars(length(min = 1, max = 2048), extend("format" = "uri"))]
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(length(min = 1, max = 16384))]
        credential: Option<String>,
    },
}

/// One Environment a session declares: a name unique within the session, how Brain
/// reaches it, and its own configuration, which Brain carries and never reads.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct Environment {
    pub name: EnvironmentName,
    #[serde(flatten)]
    pub driver: Driver,
    #[serde(default)]
    pub configuration: serde_json::Value,
}

/// One operation on an Environment, named by `(session_id, environment, sequence)`.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct EnvironmentOperation {
    /// The sequence of the journal record that started this operation. With
    /// `session_id` it names the operation: a redelivery carries the same pair, so a
    /// receiver that already answered it can say so.
    #[schemars(range(min = 1))]
    pub sequence: u64,
    pub environment: EnvironmentName,
    pub session_id: SessionId,
    pub request: EnvironmentRequest,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct EnvironmentCommand {
    #[schemars(schema_with = "crate::schema::environment_contract")]
    pub contract: String,
    pub operation: EnvironmentOperation,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct EnvironmentResponse {
    #[schemars(schema_with = "crate::schema::environment_contract")]
    pub contract: String,
    #[schemars(range(min = 1))]
    pub sequence: u64,
    pub receipt: EnvironmentReceipt,
}

/// What Brain asks an Environment to do. An Environment provides resources and learns
/// what runs in it only when asked to run it: setup carries its configuration and the
/// needs of everything placed there, each invoke carries the Tool it runs, each turn
/// the Agentloop.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EnvironmentRequest {
    Setup {
        configuration: serde_json::Value,
        /// Every need of every Tool and the Agentloop placed here, as URIs. The
        /// Environment refuses at setup what it cannot honour, naming the URI.
        #[schemars(schema_with = "crate::schema::needs")]
        needs: Vec<String>,
    },
    Call {
        #[schemars(schema_with = "crate::schema::identifier")]
        name: String,
        input: serde_json::Value,
    },
    Invoke {
        /// The Tool's name, as the session declared it.
        #[schemars(schema_with = "crate::schema::identifier")]
        tool: String,
        /// The Tool's implementation as it was declared: opaque to Brain, interpreted here.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        implementation: Option<serde_json::Value>,
        #[schemars(schema_with = "crate::schema::needs")]
        needs: Vec<String>,
        input: serde_json::Value,
        #[schemars(range(min = 1))]
        deadline_ms: u64,
    },
    Turn {
        id: AgentloopId,
        #[schemars(schema_with = "crate::schema::needs")]
        needs: Vec<String>,
        input: Box<TurnInput>,
    },
    Cancel {
        #[schemars(range(min = 1))]
        target_sequence: u64,
    },
    Detach,
    Teardown,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EnvironmentReceipt {
    Accepted,
    Progress {
        data: serde_json::Value,
    },
    Result {
        output: serde_json::Value,
    },
    Outcome {
        outcome: Outcome,
    },
    Turned {
        output: TurnOutput,
    },
    Failure {
        #[schemars(schema_with = "crate::schema::identifier")]
        code: String,
        #[schemars(length(max = 4096))]
        message: String,
        retryable: bool,
    },
    Unknown {
        #[schemars(length(max = 4096))]
        message: String,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentCallRequest {
    pub input: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentCallResult {
    pub output: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_driver_is_a_sibling_of_the_configuration() {
        let http: Environment = serde_json::from_value(serde_json::json!({
            "name": "sandbox",
            "driver": "http",
            "url": "https://sandbox.example",
            "credential": "s3cret",
            "configuration": {"region": "eu"}
        }))
        .unwrap();
        assert!(
            matches!(&http.driver, Driver::Http { url, credential: Some(credential) }
            if url == "https://sandbox.example" && credential == "s3cret")
        );
        let brain: Environment =
            serde_json::from_value(serde_json::json!({"name": "brain", "driver": "brain"}))
                .unwrap();
        assert!(matches!(brain.driver, Driver::Brain {}));
        assert_eq!(
            serde_json::to_value(&brain).unwrap(),
            serde_json::json!({"name": "brain", "driver": "brain", "configuration": null})
        );
        for wrong in [
            serde_json::json!({"name": "x", "driver": "brain", "url": "https://x"}),
            serde_json::json!({"name": "x", "driver": "http"}),
            serde_json::json!({"name": "x", "driver": "elsewhere"}),
            serde_json::json!({"name": "x", "driver": "host"}),
        ] {
            assert!(
                serde_json::from_value::<Environment>(wrong.clone()).is_err(),
                "{wrong} must be refused"
            );
        }
    }
}
