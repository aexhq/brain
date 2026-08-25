use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use async_trait::async_trait;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{agentloop, environment, model, tool};

const EPOCH_TICK: Duration = Duration::from_millis(10);
const EPOCH_DEADLINE_TICKS: u64 = 3_000;
const MEMORY_LIMIT_BYTES: usize = 256 << 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityCall {
    pub world: String,
    pub instance_id: Option<String>,
    pub capability: String,
    pub operation_id: String,
    pub request: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityFailure {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[async_trait]
pub trait CapabilityHandler: Send + Sync + 'static {
    async fn call(&self, request: CapabilityCall) -> Result<Value, CapabilityFailure>;
}

type CapabilityRouteKey = (String, String);
type CapabilityRoute = (u64, Arc<dyn CapabilityHandler>);

pub struct CapabilityRouter {
    fallback: Arc<dyn CapabilityHandler>,
    routes: RwLock<HashMap<CapabilityRouteKey, CapabilityRoute>>,
    next_token: AtomicU64,
}

impl CapabilityRouter {
    pub fn new(fallback: Arc<dyn CapabilityHandler>) -> Arc<Self> {
        Arc::new(Self {
            fallback,
            routes: RwLock::new(HashMap::new()),
            next_token: AtomicU64::new(1),
        })
    }

    pub fn bind(
        self: &Arc<Self>,
        world: impl Into<String>,
        instance_id: impl Into<String>,
        handler: Arc<dyn CapabilityHandler>,
    ) -> anyhow::Result<CapabilityBinding> {
        let world = world.into();
        let instance_id = instance_id.into();
        let key = (world.clone(), instance_id.clone());
        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
        let mut routes = self.routes.write().expect("capability routes");
        if routes.contains_key(&key) {
            anyhow::bail!("capabilities are already bound for {world} instance {instance_id}");
        }
        routes.insert(key, (token, handler));
        Ok(CapabilityBinding {
            router: self.clone(),
            world,
            instance_id,
            token,
        })
    }
}

#[async_trait]
impl CapabilityHandler for CapabilityRouter {
    async fn call(&self, request: CapabilityCall) -> Result<Value, CapabilityFailure> {
        let handler = request.instance_id.as_ref().and_then(|instance_id| {
            self.routes
                .read()
                .expect("capability routes")
                .get(&(request.world.clone(), instance_id.clone()))
                .map(|(_, handler)| handler.clone())
        });
        match handler {
            Some(handler) => handler.call(request).await,
            None => self.fallback.call(request).await,
        }
    }
}

pub struct CapabilityBinding {
    router: Arc<CapabilityRouter>,
    world: String,
    instance_id: String,
    token: u64,
}

impl Drop for CapabilityBinding {
    fn drop(&mut self) {
        let mut routes = self.router.routes.write().expect("capability routes");
        let key = (self.world.clone(), self.instance_id.clone());
        if routes.get(&key).map(|(token, _)| *token) == Some(self.token) {
            routes.remove(&key);
        }
    }
}

pub(crate) struct DenyCapabilities;

#[async_trait]
impl CapabilityHandler for DenyCapabilities {
    async fn call(&self, request: CapabilityCall) -> Result<Value, CapabilityFailure> {
        Err(CapabilityFailure {
            code: "capability_denied".into(),
            message: format!("the component was not granted {}", request.capability),
            retryable: false,
        })
    }
}

fn wt<T>(result: Result<T, wasmtime::Error>) -> anyhow::Result<T> {
    result.map_err(|error| anyhow::anyhow!(error.to_string()))
}

struct State {
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
    cancelled: bool,
    capabilities: Arc<dyn CapabilityHandler>,
    world: &'static str,
    instance_id: Option<String>,
    tool_metadata: Option<tool::aex::tool::types::CallMetadata>,
    tool_grants: HashSet<String>,
    tool_capability_sequence: u64,
}

impl State {
    fn new(
        capabilities: Arc<dyn CapabilityHandler>,
        world: &'static str,
        instance_id: Option<String>,
    ) -> Self {
        Self {
            wasi: WasiCtxBuilder::new().build(),
            table: ResourceTable::new(),
            limits: StoreLimitsBuilder::new()
                .memory_size(MEMORY_LIMIT_BYTES)
                .build(),
            cancelled: false,
            capabilities,
            world,
            instance_id,
            tool_metadata: None,
            tool_grants: HashSet::new(),
            tool_capability_sequence: 0,
        }
    }

    fn tool(
        capabilities: Arc<dyn CapabilityHandler>,
        metadata: tool::aex::tool::types::CallMetadata,
        grants: &[String],
    ) -> Self {
        let mut state = Self::new(capabilities, "tool", Some(metadata.call_id.clone()));
        state.tool_metadata = Some(metadata);
        state.tool_grants.extend(grants.iter().cloned());
        state
    }

    async fn capability(
        &mut self,
        capability: &str,
        operation_id: String,
        request: Value,
    ) -> Result<Value, CapabilityFailure> {
        self.capabilities
            .call(CapabilityCall {
                world: self.world.into(),
                instance_id: self.instance_id.clone(),
                capability: capability.into(),
                operation_id,
                request,
            })
            .await
    }

    fn has_tool_grant(&self, grant: &str) -> Result<(), CapabilityFailure> {
        if self.tool_grants.contains(grant) {
            Ok(())
        } else {
            Err(CapabilityFailure {
                code: "capability_denied".into(),
                message: format!("the Tool was not granted {grant}"),
                retryable: false,
            })
        }
    }

    async fn tool_capability(
        &mut self,
        grant: &str,
        capability: &str,
        request: Value,
    ) -> Result<Value, tool::aex::tool::types::ExtensionError> {
        self.has_tool_grant(grant).map_err(tool_error)?;
        self.tool_capability_sequence += 1;
        let call_id = &self
            .tool_metadata
            .as_ref()
            .expect("Tool metadata is installed before invocation")
            .call_id;
        let operation_id = format!("{call_id}:{}:{capability}", self.tool_capability_sequence);
        self.capability(capability, operation_id, request)
            .await
            .map_err(tool_error)
    }
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl agentloop::aex::agentloop::context::Host for State {
    async fn call(
        &mut self,
        operation_id: String,
        request_json: String,
    ) -> Result<String, agentloop::aex::agentloop::types::ExtensionError> {
        let request: Value = serde_json::from_str(&request_json).map_err(|error| {
            agentloop::aex::agentloop::types::ExtensionError {
                code: "invalid_request".into(),
                message: error.to_string(),
                retryable: false,
            }
        })?;
        let value = self
            .capability("agentloop.call", operation_id, request)
            .await
            .map_err(agentloop_error)?;
        value.as_str().map(str::to_owned).ok_or_else(|| {
            agentloop::aex::agentloop::types::ExtensionError {
                code: "invalid_response".into(),
                message: "agentloop.call response must be a string".into(),
                retryable: false,
            }
        })
    }

    async fn cancelled(&mut self) -> bool {
        self.cancelled
    }
}

impl agentloop::aex::agentloop::types::Host for State {}

fn agentloop_error(error: CapabilityFailure) -> agentloop::aex::agentloop::types::ExtensionError {
    agentloop::aex::agentloop::types::ExtensionError {
        code: error.code,
        message: error.message,
        retryable: error.retryable,
    }
}

impl tool::aex::tool::context::Host for State {
    async fn metadata(&mut self) -> tool::aex::tool::types::CallMetadata {
        self.tool_metadata
            .clone()
            .expect("Tool metadata is installed before invocation")
    }

    async fn cancelled(&mut self) -> bool {
        self.cancelled
    }

    async fn log(&mut self, _level: tool::aex::tool::types::LogLevel, _message: String) {}
}

impl tool::aex::tool::types::Host for State {}

impl tool::aex::tool::environment::Host for State {
    async fn invoke(
        &mut self,
        operation_id: String,
        descriptor_json: String,
        bundle: Option<Vec<u8>>,
        input_json: String,
        deadline_at_ms: u64,
    ) -> Result<String, tool::aex::tool::types::ExtensionError> {
        let metadata = self
            .tool_metadata
            .clone()
            .expect("Tool metadata is installed before invocation");
        let value = self
            .tool_capability(
                "environment",
                "tool.environment.invoke",
                serde_json::json!({
                    "component_operation_id": operation_id,
                    "metadata": metadata,
                    "descriptor_json": descriptor_json,
                    "bundle_base64": bundle.map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes)),
                    "input_json": input_json,
                    "deadline_at_ms": deadline_at_ms.to_string(),
                }),
            )
            .await?;
        value
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| tool::aex::tool::types::ExtensionError {
                code: "invalid_response".into(),
                message: "tool.environment.invoke response must be a string".into(),
                retryable: false,
            })
    }
}

impl tool::aex::tool::journal::Host for State {
    async fn read(
        &mut self,
        after_seq: Option<u64>,
        limit: u32,
    ) -> Result<tool::aex::tool::types::Page, tool::aex::tool::types::ExtensionError> {
        let value = self
            .tool_capability(
                "journal",
                "tool.journal.read",
                serde_json::json!({ "after_seq": after_seq, "limit": limit }),
            )
            .await?;
        tool_value(value)
    }
}

impl tool::aex::tool::storage::Host for State {
    async fn list_objects(
        &mut self,
        prefix: Option<String>,
        cursor: Option<String>,
        limit: u32,
    ) -> Result<tool::aex::tool::types::Page, tool::aex::tool::types::ExtensionError> {
        let value = self
            .tool_capability(
                "storage",
                "tool.storage.list",
                serde_json::json!({ "prefix": prefix, "cursor": cursor, "limit": limit }),
            )
            .await?;
        tool_value(value)
    }

    async fn stat(
        &mut self,
        key: String,
    ) -> Result<String, tool::aex::tool::types::ExtensionError> {
        let value = self
            .tool_capability(
                "storage",
                "tool.storage.stat",
                serde_json::json!({ "key": key }),
            )
            .await?;
        tool_string(value)
    }

    async fn read(
        &mut self,
        key: String,
        offset: u64,
        length: u32,
    ) -> Result<Vec<u8>, tool::aex::tool::types::ExtensionError> {
        let value = self
            .tool_capability(
                "storage",
                "tool.storage.read",
                serde_json::json!({ "key": key, "offset": offset, "length": length }),
            )
            .await?;
        let encoded = tool_string(value)?;
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| tool::aex::tool::types::ExtensionError {
                code: "invalid_response".into(),
                message: error.to_string(),
                retryable: false,
            })
    }

    async fn write(
        &mut self,
        key: String,
        bytes: Vec<u8>,
    ) -> Result<String, tool::aex::tool::types::ExtensionError> {
        let value = self
            .tool_capability(
                "storage",
                "tool.storage.write",
                serde_json::json!({
                    "key": key,
                    "bytes_base64": base64::engine::general_purpose::STANDARD.encode(bytes),
                }),
            )
            .await?;
        tool_string(value)
    }

    async fn delete(&mut self, key: String) -> Result<(), tool::aex::tool::types::ExtensionError> {
        self.tool_capability(
            "storage",
            "tool.storage.delete",
            serde_json::json!({ "key": key }),
        )
        .await?;
        Ok(())
    }
}

impl tool::aex::tool::children::Host for State {
    async fn spawn(
        &mut self,
        request_json: String,
    ) -> Result<String, tool::aex::tool::types::ExtensionError> {
        let value = self
            .tool_capability(
                "children",
                "tool.children.spawn",
                serde_json::json!({ "request_json": request_json }),
            )
            .await?;
        tool_string(value)
    }

    async fn send(
        &mut self,
        child_id: String,
        request_json: String,
    ) -> Result<String, tool::aex::tool::types::ExtensionError> {
        let value = self
            .tool_capability(
                "children",
                "tool.children.send",
                serde_json::json!({ "child_id": child_id, "request_json": request_json }),
            )
            .await?;
        tool_string(value)
    }

    async fn inspect(
        &mut self,
        child_id: String,
    ) -> Result<String, tool::aex::tool::types::ExtensionError> {
        let value = self
            .tool_capability(
                "children",
                "tool.children.inspect",
                serde_json::json!({ "child_id": child_id }),
            )
            .await?;
        tool_string(value)
    }

    async fn events(
        &mut self,
        child_id: String,
        after_seq: Option<u64>,
        limit: u32,
    ) -> Result<tool::aex::tool::types::Page, tool::aex::tool::types::ExtensionError> {
        let value = self
            .tool_capability(
                "children",
                "tool.children.events",
                serde_json::json!({
                    "child_id": child_id,
                    "after_seq": after_seq,
                    "limit": limit,
                }),
            )
            .await?;
        tool_value(value)
    }

    async fn manage(
        &mut self,
        child_id: String,
        action: String,
    ) -> Result<String, tool::aex::tool::types::ExtensionError> {
        let value = self
            .tool_capability(
                "children",
                "tool.children.manage",
                serde_json::json!({ "child_id": child_id, "action": action }),
            )
            .await?;
        tool_string(value)
    }

    async fn list_children(
        &mut self,
        cursor: Option<String>,
        limit: u32,
    ) -> Result<tool::aex::tool::types::Page, tool::aex::tool::types::ExtensionError> {
        let value = self
            .tool_capability(
                "children",
                "tool.children.list",
                serde_json::json!({ "cursor": cursor, "limit": limit }),
            )
            .await?;
        tool_value(value)
    }
}

impl tool::aex::tool::parent::Host for State {
    async fn metadata(&mut self) -> Result<String, tool::aex::tool::types::ExtensionError> {
        let value = self
            .tool_capability("parent", "tool.parent.metadata", serde_json::json!({}))
            .await?;
        tool_string(value)
    }

    async fn inspect(&mut self) -> Result<String, tool::aex::tool::types::ExtensionError> {
        let value = self
            .tool_capability("parent", "tool.parent.inspect", serde_json::json!({}))
            .await?;
        tool_string(value)
    }

    async fn events(
        &mut self,
        after_seq: Option<u64>,
        limit: u32,
    ) -> Result<tool::aex::tool::types::Page, tool::aex::tool::types::ExtensionError> {
        let value = self
            .tool_capability(
                "parent",
                "tool.parent.events",
                serde_json::json!({ "after_seq": after_seq, "limit": limit }),
            )
            .await?;
        tool_value(value)
    }

    async fn send(
        &mut self,
        request_json: String,
    ) -> Result<String, tool::aex::tool::types::ExtensionError> {
        let value = self
            .tool_capability(
                "parent",
                "tool.parent.send",
                serde_json::json!({ "request_json": request_json }),
            )
            .await?;
        tool_string(value)
    }
}

fn tool_error(error: CapabilityFailure) -> tool::aex::tool::types::ExtensionError {
    tool::aex::tool::types::ExtensionError {
        code: error.code,
        message: error.message,
        retryable: error.retryable,
    }
}

fn tool_value<T: serde::de::DeserializeOwned>(
    value: Value,
) -> Result<T, tool::aex::tool::types::ExtensionError> {
    serde_json::from_value(value).map_err(|error| tool::aex::tool::types::ExtensionError {
        code: "invalid_response".into(),
        message: error.to_string(),
        retryable: false,
    })
}

fn tool_string(value: Value) -> Result<String, tool::aex::tool::types::ExtensionError> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| tool::aex::tool::types::ExtensionError {
            code: "invalid_response".into(),
            message: "Tool capability response must be a string".into(),
            retryable: false,
        })
}

impl environment::aex::environment::host::Host for State {
    async fn cancelled(&mut self) -> bool {
        self.cancelled
    }

    async fn log(
        &mut self,
        _level: environment::aex::environment::types::LogLevel,
        _message: String,
    ) {
    }

    async fn dispatch(
        &mut self,
        operation_id: String,
        action: String,
        request_json: String,
        deadline_at_ms: u64,
    ) -> Result<String, environment::aex::environment::types::ExtensionError> {
        let request: Value = serde_json::from_str(&request_json).map_err(|error| {
            environment::aex::environment::types::ExtensionError {
                code: "invalid_request".into(),
                message: error.to_string(),
                retryable: false,
            }
        })?;
        let value = self
            .capability(
                "environment.dispatch",
                operation_id,
                serde_json::json!({
                    "action": action,
                    "request": request,
                    "deadline_at_ms": deadline_at_ms.to_string(),
                }),
            )
            .await
            .map_err(environment_error)?;
        serde_json::to_string(&value).map_err(|error| {
            environment::aex::environment::types::ExtensionError {
                code: "invalid_response".into(),
                message: error.to_string(),
                retryable: false,
            }
        })
    }

    async fn http(
        &mut self,
        request: environment::aex::environment::types::HttpRequest,
    ) -> Result<
        environment::aex::environment::types::HttpResponse,
        environment::aex::environment::types::ExtensionError,
    > {
        let operation_id = format!("http:{}", request.deadline_at_ms);
        let value = self
            .capability(
                "environment.http",
                operation_id,
                serde_json::to_value(request).expect("generated HTTP request serializes"),
            )
            .await
            .map_err(environment_error)?;
        serde_json::from_value(value).map_err(|error| {
            environment::aex::environment::types::ExtensionError {
                code: "invalid_response".into(),
                message: error.to_string(),
                retryable: false,
            }
        })
    }
}

impl environment::aex::environment::types::Host for State {}

fn environment_error(
    error: CapabilityFailure,
) -> environment::aex::environment::types::ExtensionError {
    environment::aex::environment::types::ExtensionError {
        code: error.code,
        message: error.message,
        retryable: error.retryable,
    }
}

impl model::aex::model::host::Host for State {
    async fn cancelled(&mut self) -> bool {
        self.cancelled
    }

    async fn log(&mut self, _level: model::aex::model::types::LogLevel, _message: String) {}

    async fn http_start(
        &mut self,
        operation_id: String,
        request: model::aex::model::types::HttpRequest,
    ) -> Result<model::aex::model::types::HttpStarted, model::aex::model::types::ExtensionError>
    {
        let value = self
            .capability(
                "model.http.start",
                operation_id,
                serde_json::to_value(request).expect("generated HTTP request serializes"),
            )
            .await
            .map_err(model_error)?;
        serde_json::from_value(value).map_err(|error| model::aex::model::types::ExtensionError {
            code: "invalid_response".into(),
            message: error.to_string(),
            retryable: false,
        })
    }

    async fn http_read(
        &mut self,
        request_id: String,
        cursor: Option<String>,
        max_bytes: u32,
    ) -> Result<model::aex::model::types::HttpChunk, model::aex::model::types::ExtensionError> {
        let value = self
            .capability(
                "model.http.read",
                request_id.clone(),
                serde_json::json!({
                    "request_id": request_id,
                    "cursor": cursor,
                    "max_bytes": max_bytes,
                }),
            )
            .await
            .map_err(model_error)?;
        serde_json::from_value(value).map_err(|error| model::aex::model::types::ExtensionError {
            code: "invalid_response".into(),
            message: error.to_string(),
            retryable: false,
        })
    }

    async fn http_cancel(
        &mut self,
        request_id: String,
    ) -> Result<(), model::aex::model::types::ExtensionError> {
        self.capability(
            "model.http.cancel",
            request_id.clone(),
            serde_json::json!({ "request_id": request_id }),
        )
        .await
        .map_err(model_error)?;
        Ok(())
    }
}

impl model::aex::model::types::Host for State {}

fn model_error(error: CapabilityFailure) -> model::aex::model::types::ExtensionError {
    model::aex::model::types::ExtensionError {
        code: error.code,
        message: error.message,
        retryable: error.retryable,
    }
}

pub struct ComponentRuntime {
    engine: Engine,
    components: Mutex<HashMap<String, Component>>,
    capabilities: Arc<dyn CapabilityHandler>,
}

pub struct AgentloopInstance {
    store: Store<State>,
    bindings: agentloop::Agentloop,
}

pub struct EnvironmentInstance {
    store: Store<State>,
    bindings: environment::Environment,
}

pub struct ModelInstance {
    store: Store<State>,
    bindings: model::Model,
}

impl ModelInstance {
    pub async fn start(
        &mut self,
        request: &model::aex::model::types::Request,
    ) -> anyhow::Result<model::aex::model::types::Started> {
        wt(self.bindings.call_start(&mut self.store, request).await)?
            .map_err(|error| anyhow::anyhow!(error.message))
    }

    pub async fn observe(
        &mut self,
        provider_operation_id: &str,
        cursor: Option<&str>,
    ) -> anyhow::Result<model::aex::model::types::Observation> {
        wt(self
            .bindings
            .call_observe(&mut self.store, provider_operation_id, cursor)
            .await)?
        .map_err(|error| anyhow::anyhow!(error.message))
    }

    pub async fn cancel(&mut self, provider_operation_id: &str) -> anyhow::Result<()> {
        wt(self
            .bindings
            .call_cancel(&mut self.store, provider_operation_id)
            .await)?
        .map_err(|error| anyhow::anyhow!(error.message))
    }

    pub async fn acknowledge(
        &mut self,
        provider_operation_id: &str,
        terminal_json: &str,
    ) -> anyhow::Result<()> {
        wt(self
            .bindings
            .call_acknowledge(&mut self.store, provider_operation_id, terminal_json)
            .await)?
        .map_err(|error| anyhow::anyhow!(error.message))
    }
}

impl EnvironmentInstance {
    pub async fn resolve(
        &mut self,
        request: &environment::aex::environment::types::ResolveRequest,
    ) -> anyhow::Result<environment::aex::environment::types::Resolved> {
        wt(self.bindings.call_resolve(&mut self.store, request).await)?
            .map_err(|error| anyhow::anyhow!(error.message))
    }

    pub async fn submit(
        &mut self,
        binding_json: &str,
        operation: &environment::aex::environment::types::Operation,
    ) -> anyhow::Result<environment::aex::environment::types::Submitted> {
        wt(self
            .bindings
            .call_submit(&mut self.store, binding_json, operation)
            .await)?
        .map_err(|error| anyhow::anyhow!(error.message))
    }

    pub async fn observe(
        &mut self,
        binding_json: &str,
        provider_operation_id: &str,
        cursor: Option<&str>,
    ) -> anyhow::Result<environment::aex::environment::types::Observation> {
        wt(self
            .bindings
            .call_observe(&mut self.store, binding_json, provider_operation_id, cursor)
            .await)?
        .map_err(|error| anyhow::anyhow!(error.message))
    }

    pub async fn cancel(
        &mut self,
        binding_json: &str,
        provider_operation_id: &str,
    ) -> anyhow::Result<()> {
        wt(self
            .bindings
            .call_cancel(&mut self.store, binding_json, provider_operation_id)
            .await)?
        .map_err(|error| anyhow::anyhow!(error.message))
    }

    pub async fn acknowledge(
        &mut self,
        binding_json: &str,
        provider_operation_id: &str,
        terminal_json: &str,
    ) -> anyhow::Result<()> {
        wt(self
            .bindings
            .call_acknowledge(
                &mut self.store,
                binding_json,
                provider_operation_id,
                terminal_json,
            )
            .await)?
        .map_err(|error| anyhow::anyhow!(error.message))
    }

    pub async fn release(&mut self, binding_json: &str) -> anyhow::Result<()> {
        wt(self
            .bindings
            .call_release(&mut self.store, binding_json)
            .await)?
        .map_err(|error| anyhow::anyhow!(error.message))
    }
}

impl AgentloopInstance {
    pub async fn activate(
        &mut self,
        request: &agentloop::aex::agentloop::types::Activation,
    ) -> anyhow::Result<agentloop::aex::agentloop::types::ActivationResult> {
        wt(self.bindings.call_activate(&mut self.store, request).await)?
            .map_err(|error| anyhow::anyhow!(error.message))
    }
}

impl ComponentRuntime {
    pub fn new() -> anyhow::Result<Arc<Self>> {
        Self::with_capabilities(Arc::new(DenyCapabilities))
    }

    pub fn with_capabilities(
        capabilities: Arc<dyn CapabilityHandler>,
    ) -> anyhow::Result<Arc<Self>> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.epoch_interruption(true);
        let engine = wt(Engine::new(&config))?;
        let ticker = engine.clone();
        std::thread::Builder::new()
            .name("component-host-epoch".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(EPOCH_TICK);
                    ticker.increment_epoch();
                }
            })?;
        Ok(Arc::new(Self {
            engine,
            components: Mutex::new(HashMap::new()),
            capabilities,
        }))
    }

    fn store_with(&self, state: State) -> Store<State> {
        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| &mut state.limits);
        store.set_epoch_deadline(EPOCH_DEADLINE_TICKS);
        store
    }

    fn component(&self, bytes: &[u8]) -> anyhow::Result<Component> {
        let digest = component_digest(bytes);
        if let Some(component) = self.components.lock().expect("components").get(&digest) {
            return Ok(component.clone());
        }
        let component = wt(Component::new(&self.engine, bytes))?;
        self.components
            .lock()
            .expect("components")
            .insert(digest, component.clone());
        Ok(component)
    }

    pub async fn invoke_agentloop(
        &self,
        bytes: &[u8],
        request: agentloop::aex::agentloop::types::Activation,
    ) -> anyhow::Result<agentloop::aex::agentloop::types::ActivationResult> {
        self.instantiate_agentloop(bytes)
            .await?
            .activate(&request)
            .await
    }

    pub async fn instantiate_agentloop(&self, bytes: &[u8]) -> anyhow::Result<AgentloopInstance> {
        self.instantiate_agentloop_scoped(bytes, None).await
    }

    pub async fn instantiate_agentloop_scoped(
        &self,
        bytes: &[u8],
        instance_id: Option<String>,
    ) -> anyhow::Result<AgentloopInstance> {
        let component = self.component(bytes)?;
        let mut linker = Linker::new(&self.engine);
        wt(wasmtime_wasi::p2::add_to_linker_async(&mut linker))?;
        wt(
            agentloop::Agentloop::add_to_linker::<State, HasSelf<State>>(&mut linker, |state| {
                state
            }),
        )?;
        let mut store = self.store_with(State::new(
            self.capabilities.clone(),
            "agentloop",
            instance_id,
        ));
        let bindings =
            wt(agentloop::Agentloop::instantiate_async(&mut store, &component, &linker).await)?;
        Ok(AgentloopInstance { store, bindings })
    }

    pub async fn invoke_tool(
        &self,
        bytes: &[u8],
        request: tool::aex::tool::types::Invocation,
    ) -> anyhow::Result<tool::aex::tool::types::Outcome> {
        self.invoke_tool_granted(bytes, request, &[]).await
    }

    pub async fn invoke_tool_granted(
        &self,
        bytes: &[u8],
        request: tool::aex::tool::types::Invocation,
        grants: &[String],
    ) -> anyhow::Result<tool::aex::tool::types::Outcome> {
        let component = self.component(bytes)?;
        let mut linker = Linker::new(&self.engine);
        wt(wasmtime_wasi::p2::add_to_linker_async(&mut linker))?;
        wt(tool::Tool::add_to_linker::<State, HasSelf<State>>(
            &mut linker,
            |state| state,
        ))?;
        let mut store = self.store_with(State::tool(
            self.capabilities.clone(),
            request.metadata.clone(),
            grants,
        ));
        let bindings = wt(tool::Tool::instantiate_async(&mut store, &component, &linker).await)?;
        wt(bindings.call_invoke(&mut store, &request).await)?
            .map_err(|error| anyhow::anyhow!(error.message))
    }

    pub async fn resolve_environment(
        &self,
        bytes: &[u8],
        request: environment::aex::environment::types::ResolveRequest,
    ) -> anyhow::Result<environment::aex::environment::types::Resolved> {
        self.instantiate_environment(bytes)
            .await?
            .resolve(&request)
            .await
    }

    pub async fn instantiate_environment(
        &self,
        bytes: &[u8],
    ) -> anyhow::Result<EnvironmentInstance> {
        self.instantiate_environment_scoped(bytes, None).await
    }

    pub async fn instantiate_environment_scoped(
        &self,
        bytes: &[u8],
        instance_id: Option<String>,
    ) -> anyhow::Result<EnvironmentInstance> {
        let component = self.component(bytes)?;
        let mut linker = Linker::new(&self.engine);
        wt(wasmtime_wasi::p2::add_to_linker_async(&mut linker))?;
        wt(environment::Environment::add_to_linker::<
            State,
            HasSelf<State>,
        >(&mut linker, |state| state))?;
        let mut store = self.store_with(State::new(
            self.capabilities.clone(),
            "environment",
            instance_id,
        ));
        let bindings =
            wt(environment::Environment::instantiate_async(&mut store, &component, &linker).await)?;
        Ok(EnvironmentInstance { store, bindings })
    }

    pub async fn exercise_environment(
        &self,
        bytes: &[u8],
        request: environment::aex::environment::types::ResolveRequest,
        operation: environment::aex::environment::types::Operation,
    ) -> anyhow::Result<environment::aex::environment::types::Observation> {
        let mut instance = self.instantiate_environment(bytes).await?;
        let resolved = instance.resolve(&request).await?;
        let submitted = instance.submit(&resolved.binding_json, &operation).await?;
        let observation = instance
            .observe(
                &resolved.binding_json,
                &submitted.provider_operation_id,
                None,
            )
            .await?;
        let terminal = observation
            .terminal_json
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("deterministic Environment returned no terminal"))?;
        instance
            .acknowledge(
                &resolved.binding_json,
                &submitted.provider_operation_id,
                terminal,
            )
            .await?;
        instance.release(&resolved.binding_json).await?;
        Ok(observation)
    }

    pub async fn start_model(
        &self,
        bytes: &[u8],
        request: model::aex::model::types::Request,
    ) -> anyhow::Result<model::aex::model::types::Started> {
        self.instantiate_model(bytes).await?.start(&request).await
    }

    pub async fn instantiate_model(&self, bytes: &[u8]) -> anyhow::Result<ModelInstance> {
        self.instantiate_model_scoped(bytes, None).await
    }

    pub async fn instantiate_model_scoped(
        &self,
        bytes: &[u8],
        instance_id: Option<String>,
    ) -> anyhow::Result<ModelInstance> {
        let component = self.component(bytes)?;
        let mut linker = Linker::new(&self.engine);
        wt(wasmtime_wasi::p2::add_to_linker_async(&mut linker))?;
        wt(model::Model::add_to_linker::<State, HasSelf<State>>(
            &mut linker,
            |state| state,
        ))?;
        let mut store =
            self.store_with(State::new(self.capabilities.clone(), "model", instance_id));
        let bindings = wt(model::Model::instantiate_async(&mut store, &component, &linker).await)?;
        Ok(ModelInstance { store, bindings })
    }

    pub async fn exercise_model(
        &self,
        bytes: &[u8],
        request: model::aex::model::types::Request,
    ) -> anyhow::Result<model::aex::model::types::Observation> {
        let mut instance = self.instantiate_model(bytes).await?;
        let started = instance.start(&request).await?;
        let observation = instance
            .observe(&started.provider_operation_id, None)
            .await?;
        let terminal = observation
            .terminal_json
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("deterministic Model returned no terminal"))?;
        instance
            .acknowledge(&started.provider_operation_id, terminal)
            .await?;
        Ok(observation)
    }
}

pub fn component_digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
