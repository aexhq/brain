use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use sha2::{Digest, Sha256};

use crate::{agentloop, environment, model, tool};

const EPOCH_TICK: Duration = Duration::from_millis(10);
const EPOCH_DEADLINE_TICKS: u64 = 3_000;
const MEMORY_LIMIT_BYTES: usize = 256 << 20;

fn wt<T>(result: Result<T, wasmtime::Error>) -> anyhow::Result<T> {
    result.map_err(|error| anyhow::anyhow!(error.to_string()))
}

struct State {
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
    cancelled: bool,
}

impl State {
    fn new() -> Self {
        Self {
            wasi: WasiCtxBuilder::new().build(),
            table: ResourceTable::new(),
            limits: StoreLimitsBuilder::new()
                .memory_size(MEMORY_LIMIT_BYTES)
                .build(),
            cancelled: false,
        }
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
        _operation_id: String,
        request_json: String,
    ) -> Result<String, agentloop::aex::agentloop::types::ExtensionError> {
        Ok(request_json)
    }

    async fn cancelled(&mut self) -> bool {
        self.cancelled
    }
}

impl agentloop::aex::agentloop::types::Host for State {}

impl tool::aex::tool::context::Host for State {
    async fn metadata(&mut self) -> tool::aex::tool::types::CallMetadata {
        tool::aex::tool::types::CallMetadata {
            tenant_id: "tenant_test".into(),
            session_id: "ses_test".into(),
            turn_id: "turn_test".into(),
            call_id: "call_test".into(),
            tool_name: "echo".into(),
        }
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
        _operation_id: String,
        _descriptor_json: String,
        _bundle: Option<Vec<u8>>,
        _input_json: String,
        _deadline_at_ms: u64,
    ) -> Result<String, tool::aex::tool::types::ExtensionError> {
        Err(tool_denied("environment"))
    }
}

impl tool::aex::tool::journal::Host for State {
    async fn read(
        &mut self,
        _after_seq: Option<u64>,
        _limit: u32,
    ) -> Result<tool::aex::tool::types::Page, tool::aex::tool::types::ExtensionError> {
        Err(tool_denied("journal"))
    }
}

impl tool::aex::tool::storage::Host for State {
    async fn list_objects(
        &mut self,
        _prefix: Option<String>,
        _cursor: Option<String>,
        _limit: u32,
    ) -> Result<tool::aex::tool::types::Page, tool::aex::tool::types::ExtensionError> {
        Err(tool_denied("storage"))
    }

    async fn stat(
        &mut self,
        _key: String,
    ) -> Result<String, tool::aex::tool::types::ExtensionError> {
        Err(tool_denied("storage"))
    }

    async fn read(
        &mut self,
        _key: String,
        _offset: u64,
        _length: u32,
    ) -> Result<Vec<u8>, tool::aex::tool::types::ExtensionError> {
        Err(tool_denied("storage"))
    }

    async fn write(
        &mut self,
        _key: String,
        _bytes: Vec<u8>,
    ) -> Result<String, tool::aex::tool::types::ExtensionError> {
        Err(tool_denied("storage"))
    }

    async fn delete(&mut self, _key: String) -> Result<(), tool::aex::tool::types::ExtensionError> {
        Err(tool_denied("storage"))
    }
}

impl tool::aex::tool::children::Host for State {
    async fn spawn(
        &mut self,
        _request_json: String,
    ) -> Result<String, tool::aex::tool::types::ExtensionError> {
        Err(tool_denied("children"))
    }

    async fn send(
        &mut self,
        _child_id: String,
        _request_json: String,
    ) -> Result<String, tool::aex::tool::types::ExtensionError> {
        Err(tool_denied("children"))
    }

    async fn inspect(
        &mut self,
        _child_id: String,
    ) -> Result<String, tool::aex::tool::types::ExtensionError> {
        Err(tool_denied("children"))
    }

    async fn events(
        &mut self,
        _child_id: String,
        _after_seq: Option<u64>,
        _limit: u32,
    ) -> Result<tool::aex::tool::types::Page, tool::aex::tool::types::ExtensionError> {
        Err(tool_denied("children"))
    }

    async fn manage(
        &mut self,
        _child_id: String,
        _action: String,
    ) -> Result<String, tool::aex::tool::types::ExtensionError> {
        Err(tool_denied("children"))
    }

    async fn list_children(
        &mut self,
        _cursor: Option<String>,
        _limit: u32,
    ) -> Result<tool::aex::tool::types::Page, tool::aex::tool::types::ExtensionError> {
        Err(tool_denied("children"))
    }
}

impl tool::aex::tool::parent::Host for State {
    async fn metadata(&mut self) -> Result<String, tool::aex::tool::types::ExtensionError> {
        Err(tool_denied("parent"))
    }

    async fn inspect(&mut self) -> Result<String, tool::aex::tool::types::ExtensionError> {
        Err(tool_denied("parent"))
    }

    async fn events(
        &mut self,
        _after_seq: Option<u64>,
        _limit: u32,
    ) -> Result<tool::aex::tool::types::Page, tool::aex::tool::types::ExtensionError> {
        Err(tool_denied("parent"))
    }

    async fn send(
        &mut self,
        _request_json: String,
    ) -> Result<String, tool::aex::tool::types::ExtensionError> {
        Err(tool_denied("parent"))
    }
}

fn tool_denied(capability: &str) -> tool::aex::tool::types::ExtensionError {
    tool::aex::tool::types::ExtensionError {
        code: "capability_denied".into(),
        message: format!("the Tool was not granted {capability}"),
        retryable: false,
    }
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
        _operation_id: String,
        _action: String,
        _request_json: String,
        _deadline_at_ms: u64,
    ) -> Result<String, environment::aex::environment::types::ExtensionError> {
        Err(environment::aex::environment::types::ExtensionError {
            code: "driver_denied".into(),
            message: "the deterministic host grants no Environment driver".into(),
            retryable: false,
        })
    }

    async fn http(
        &mut self,
        _request: environment::aex::environment::types::HttpRequest,
    ) -> Result<
        environment::aex::environment::types::HttpResponse,
        environment::aex::environment::types::ExtensionError,
    > {
        Err(environment::aex::environment::types::ExtensionError {
            code: "network_denied".into(),
            message: "the deterministic host grants no network authority".into(),
            retryable: false,
        })
    }
}

impl environment::aex::environment::types::Host for State {}

impl model::aex::model::host::Host for State {
    async fn cancelled(&mut self) -> bool {
        self.cancelled
    }

    async fn log(&mut self, _level: model::aex::model::types::LogLevel, _message: String) {}

    async fn http(
        &mut self,
        _request: model::aex::model::types::HttpRequest,
    ) -> Result<model::aex::model::types::HttpResponse, model::aex::model::types::ExtensionError>
    {
        Err(model::aex::model::types::ExtensionError {
            code: "network_denied".into(),
            message: "the deterministic host grants no network authority".into(),
            retryable: false,
        })
    }
}

impl model::aex::model::types::Host for State {}

pub struct ComponentRuntime {
    engine: Engine,
    components: Mutex<HashMap<String, Component>>,
}

impl ComponentRuntime {
    pub fn new() -> anyhow::Result<Arc<Self>> {
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
        }))
    }

    fn store(&self) -> Store<State> {
        let mut store = Store::new(&self.engine, State::new());
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
        let component = self.component(bytes)?;
        let mut linker = Linker::new(&self.engine);
        wt(wasmtime_wasi::p2::add_to_linker_async(&mut linker))?;
        wt(
            agentloop::Agentloop::add_to_linker::<State, HasSelf<State>>(&mut linker, |state| {
                state
            }),
        )?;
        let mut store = self.store();
        let bindings =
            wt(agentloop::Agentloop::instantiate_async(&mut store, &component, &linker).await)?;
        wt(bindings.call_activate(&mut store, &request).await)?
            .map_err(|error| anyhow::anyhow!(error.message))
    }

    pub async fn invoke_tool(
        &self,
        bytes: &[u8],
        request: tool::aex::tool::types::Invocation,
    ) -> anyhow::Result<tool::aex::tool::types::Outcome> {
        let component = self.component(bytes)?;
        let mut linker = Linker::new(&self.engine);
        wt(wasmtime_wasi::p2::add_to_linker_async(&mut linker))?;
        wt(tool::Tool::add_to_linker::<State, HasSelf<State>>(
            &mut linker,
            |state| state,
        ))?;
        let mut store = self.store();
        let bindings = wt(tool::Tool::instantiate_async(&mut store, &component, &linker).await)?;
        wt(bindings.call_invoke(&mut store, &request).await)?
            .map_err(|error| anyhow::anyhow!(error.message))
    }

    pub async fn resolve_environment(
        &self,
        bytes: &[u8],
        request: environment::aex::environment::types::ResolveRequest,
    ) -> anyhow::Result<environment::aex::environment::types::Resolved> {
        let component = self.component(bytes)?;
        let mut linker = Linker::new(&self.engine);
        wt(wasmtime_wasi::p2::add_to_linker_async(&mut linker))?;
        wt(environment::Environment::add_to_linker::<
            State,
            HasSelf<State>,
        >(&mut linker, |state| state))?;
        let mut store = self.store();
        let bindings =
            wt(environment::Environment::instantiate_async(&mut store, &component, &linker).await)?;
        wt(bindings.call_resolve(&mut store, &request).await)?
            .map_err(|error| anyhow::anyhow!(error.message))
    }

    pub async fn exercise_environment(
        &self,
        bytes: &[u8],
        request: environment::aex::environment::types::ResolveRequest,
        operation: environment::aex::environment::types::Operation,
    ) -> anyhow::Result<environment::aex::environment::types::Observation> {
        let component = self.component(bytes)?;
        let mut linker = Linker::new(&self.engine);
        wt(wasmtime_wasi::p2::add_to_linker_async(&mut linker))?;
        wt(environment::Environment::add_to_linker::<
            State,
            HasSelf<State>,
        >(&mut linker, |state| state))?;
        let mut store = self.store();
        let bindings =
            wt(environment::Environment::instantiate_async(&mut store, &component, &linker).await)?;
        let resolved = wt(bindings.call_resolve(&mut store, &request).await)?
            .map_err(|error| anyhow::anyhow!(error.message))?;
        let submitted = wt(bindings
            .call_submit(&mut store, &resolved.binding_json, &operation)
            .await)?
        .map_err(|error| anyhow::anyhow!(error.message))?;
        let observation = wt(bindings
            .call_observe(
                &mut store,
                &resolved.binding_json,
                &submitted.provider_operation_id,
                None,
            )
            .await)?
        .map_err(|error| anyhow::anyhow!(error.message))?;
        let terminal = observation
            .terminal_json
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("deterministic Environment returned no terminal"))?;
        wt(bindings
            .call_acknowledge(
                &mut store,
                &resolved.binding_json,
                &submitted.provider_operation_id,
                terminal,
            )
            .await)?
        .map_err(|error| anyhow::anyhow!(error.message))?;
        wt(bindings
            .call_release(&mut store, &resolved.binding_json)
            .await)?
        .map_err(|error| anyhow::anyhow!(error.message))?;
        Ok(observation)
    }

    pub async fn start_model(
        &self,
        bytes: &[u8],
        request: model::aex::model::types::Request,
    ) -> anyhow::Result<model::aex::model::types::Started> {
        let component = self.component(bytes)?;
        let mut linker = Linker::new(&self.engine);
        wt(wasmtime_wasi::p2::add_to_linker_async(&mut linker))?;
        wt(model::Model::add_to_linker::<State, HasSelf<State>>(
            &mut linker,
            |state| state,
        ))?;
        let mut store = self.store();
        let bindings = wt(model::Model::instantiate_async(&mut store, &component, &linker).await)?;
        wt(bindings.call_start(&mut store, &request).await)?
            .map_err(|error| anyhow::anyhow!(error.message))
    }

    pub async fn exercise_model(
        &self,
        bytes: &[u8],
        request: model::aex::model::types::Request,
    ) -> anyhow::Result<model::aex::model::types::Observation> {
        let component = self.component(bytes)?;
        let mut linker = Linker::new(&self.engine);
        wt(wasmtime_wasi::p2::add_to_linker_async(&mut linker))?;
        wt(model::Model::add_to_linker::<State, HasSelf<State>>(
            &mut linker,
            |state| state,
        ))?;
        let mut store = self.store();
        let bindings = wt(model::Model::instantiate_async(&mut store, &component, &linker).await)?;
        let started = wt(bindings.call_start(&mut store, &request).await)?
            .map_err(|error| anyhow::anyhow!(error.message))?;
        let observation = wt(bindings
            .call_observe(&mut store, &started.provider_operation_id, None)
            .await)?
        .map_err(|error| anyhow::anyhow!(error.message))?;
        let terminal = observation
            .terminal_json
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("deterministic Model returned no terminal"))?;
        wt(bindings
            .call_acknowledge(&mut store, &started.provider_operation_id, terminal)
            .await)?
        .map_err(|error| anyhow::anyhow!(error.message))?;
        Ok(observation)
    }
}

pub fn component_digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
