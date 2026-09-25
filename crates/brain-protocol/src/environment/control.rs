use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::EnvironmentName;

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentLifecycle {
    Automatic,
    Manual,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentRef {
    pub name: EnvironmentName,
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentPermission {
    Read,
    Create,
    Setup,
    Update,
    Delete,
    Call,
}

/// Authority over a declared binding and instances made from its template.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentGrant {
    pub environment: EnvironmentName,
    pub permissions: Vec<EnvironmentPermission>,
    #[serde(default)]
    pub methods: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentMethod {
    #[serde(default)]
    pub effect: EnvironmentMethodEffect,
    pub description: String,
    pub input_schema: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentMethodEffect {
    #[default]
    None,
    Replace,
}

/// Fixed at admission. Provider configuration is validated, never interpreted by Brain.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentTemplate {
    #[schemars(range(min = 1))]
    pub max_instances: usize,
    pub configuration_schema: Value,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentState {
    Declared,
    Starting,
    Ready,
    Failed,
    Unknown,
    Deleting,
    Deleted,
    Detached,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentView {
    pub reference: EnvironmentRef,
    pub template: EnvironmentName,
    pub lifecycle: EnvironmentLifecycle,
    pub state: EnvironmentState,
    pub configuration: Value,
    pub methods: std::collections::BTreeMap<String, EnvironmentMethod>,
    pub tools: Vec<crate::ToolDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_operation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability: Option<EnvironmentAvailability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation: Option<EnvironmentObservation>,
}

/// The same service request in every extension role and transport.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum EnvironmentControlRequest {
    List,
    Get {
        environment: EnvironmentRef,
    },
    Create {
        template: EnvironmentName,
        name: EnvironmentName,
        configuration: Value,
    },
    Setup {
        environment: EnvironmentRef,
    },
    Update {
        environment: EnvironmentRef,
        configuration: Value,
    },
    Delete {
        environment: EnvironmentRef,
    },
    Call {
        environment: EnvironmentRef,
        method: String,
        input: Value,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentAvailability {
    Available,
    Unavailable,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum EnvironmentObservation {
    Environment {
        availability: EnvironmentAvailability,
        message: String,
    },
    Resource {
        resource: String,
        code: String,
        message: String,
    },
    Operation {
        sequence: u64,
        resolution: EnvironmentResolution,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum EnvironmentResolution {
    Ready,
    Deleted,
    Detached,
    Failed { error: super::OutcomeError },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentOutput {
    pub observation: EnvironmentObservation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EnvironmentEvent {
    Event { event_type: String, data: Value },
    Result { output: EnvironmentOutput },
}
