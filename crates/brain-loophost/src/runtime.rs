use std::{future::Future, sync::Arc, time::Duration};

use brain_protocol::{AgentloopIdentity, ToolIdentity, TurnError, TurnInput, TurnOutput, codes};
use http_body_util::BodyExt as _;
use sha2::{Digest as _, Sha256};
use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Cache, Config, Engine, Store, StoreLimits, StoreLimitsBuilder, Trap};
use wasmtime_wasi::{FsPerms, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
use wasmtime_wasi_http::{
    Error as HttpError, RequestOptions, WasiBody, WasiHttpCtx, WasiHttpCtxView, WasiHttpHooks,
    WasiHttpView,
};

use crate::{HostCall, LoopLimits, NativeEnvironment};

/// A long-running invocation yields to Tokio at this interval while retaining its fixed
/// total fuel budget.
const FUEL_YIELD_INTERVAL: u64 = 10_000_000;

mod bindings {
    wasmtime::component::bindgen!({
        path: "../../contracts/agentloop/v1",
        world: "agentloop",
        imports: { default: async },
        exports: { default: async },
    });
}

mod tool_bindings {
    wasmtime::component::bindgen!({
        path: "../../contracts/tool/v1",
        world: "tool",
        imports: { default: async },
        exports: { default: async },
    });
}

use bindings::brain::agentloop::types as wit;

/// What answers the guest's host calls while a turn runs.
#[async_trait::async_trait]
pub trait GuestHost: Send + Sync {
    async fn call(&self, call: HostCall) -> Result<String, TurnError>;
}

pub struct AdmissionEngine {
    engine: Engine,
    limits: LoopLimits,
    allowed_imports: Vec<String>,
}

pub struct AdmittedAgentloop {
    pub digest: AgentloopIdentity,
    pub component: Arc<Component>,
}

pub struct AdmittedTool {
    pub digest: ToolIdentity,
    pub component: Arc<Component>,
}

pub struct NativeToolInput {
    pub call_id: String,
    pub input: serde_json::Value,
    pub configuration: serde_json::Value,
    pub deadline_at_ms: u64,
}

impl AdmissionEngine {
    pub fn new(limits: LoopLimits, allowed_imports: Vec<String>) -> Result<Self, String> {
        let mut config = Config::new();
        config.wasm_component_model(true).consume_fuel(true);
        let cache = Cache::from_file(None).map_err(|error| {
            format!("failed to configure the Wasmtime compilation cache: {error}")
        })?;
        config.cache(Some(cache));
        let engine = Engine::new(&config).map_err(|error| error.to_string())?;
        Ok(Self {
            engine,
            limits,
            allowed_imports,
        })
    }

    pub fn admit(&self, package_bytes: &[u8]) -> Result<AdmittedAgentloop, String> {
        if package_bytes.len() > self.limits.package_bytes {
            return Err("Agentloop package exceeds the configured admission limit".into());
        }
        let actual = AgentloopIdentity::new(hex_digest(package_bytes));
        let component = Component::new(&self.engine, package_bytes)
            .map_err(|error| format!("Agentloop component is invalid: {error}"))?;
        for (name, _) in component.component_type().imports(&self.engine) {
            if !self.allowed_imports.iter().any(|allowed| name == allowed) {
                return Err(format!("Agentloop import {name:?} is not allowed"));
            }
        }
        if component
            .component_type()
            .get_export(&self.engine, "turn")
            .is_none()
        {
            return Err("Agentloop component does not export turn".into());
        }
        let mut linker = Linker::<HostState>::new(&self.engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(|error| error.to_string())?;
        wasmtime_wasi_http::p2::add_only_http_to_linker_async(&mut linker)
            .map_err(|error| error.to_string())?;
        bindings::brain::agentloop::host::add_to_linker::<_, HasSelf<HostState>>(
            &mut linker,
            |state| state,
        )
        .map_err(|error| error.to_string())?;
        let instance = linker
            .instantiate_pre(&component)
            .map_err(|error| format!("Agentloop imports do not match its world: {error}"))?;
        bindings::AgentloopPre::new(instance)
            .map_err(|error| format!("Agentloop exports do not match its world: {error}"))?;
        Ok(AdmittedAgentloop {
            digest: actual,
            component: Arc::new(component),
        })
    }

    pub fn admit_tool(&self, component_bytes: &[u8]) -> Result<AdmittedTool, String> {
        if component_bytes.len() > self.limits.package_bytes {
            return Err("Tool Component exceeds the configured admission limit".into());
        }
        let digest = ToolIdentity::new(hex_digest(component_bytes));
        let component = Component::new(&self.engine, component_bytes)
            .map_err(|error| format!("Tool Component is invalid: {error}"))?;
        for (name, _) in component.component_type().imports(&self.engine) {
            if !crate::TOOL_IMPORTS.contains(&name) && !crate::CAPABILITY_IMPORTS.contains(&name) {
                return Err(format!("Tool import {name:?} is not allowed"));
            }
        }
        if component
            .component_type()
            .get_export(&self.engine, "run")
            .is_none()
        {
            return Err("Tool Component does not export run".into());
        }
        let mut linker = Linker::<HostState>::new(&self.engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(|error| error.to_string())?;
        wasmtime_wasi_http::p2::add_only_http_to_linker_async(&mut linker)
            .map_err(|error| error.to_string())?;
        tool_bindings::brain::tool::host::add_to_linker::<_, HasSelf<HostState>>(
            &mut linker,
            |state| state,
        )
        .map_err(|error| error.to_string())?;
        let instance = linker
            .instantiate_pre(&component)
            .map_err(|error| format!("Tool imports do not match its world: {error}"))?;
        tool_bindings::ToolPre::new(instance)
            .map_err(|error| format!("Tool exports do not match its world: {error}"))?;
        Ok(AdmittedTool {
            digest,
            component: Arc::new(component),
        })
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }
    pub fn limits(&self) -> &LoopLimits {
        &self.limits
    }
}

/// The store's data: the memory limiter, bridge, and explicitly granted capabilities.
pub struct HostState {
    limits: StoreLimits,
    bridge: Option<Arc<dyn GuestHost>>,
    table: ResourceTable,
    wasi: WasiCtx,
    http: WasiHttpCtx,
    network: NetworkHooks,
    _scratch: tempfile::TempDir,
    _secrets: tempfile::TempDir,
}

impl HostState {
    fn new(
        limits: StoreLimits,
        bridge: Arc<dyn GuestHost>,
        environment: NativeEnvironment,
    ) -> Result<Self, TurnError> {
        let scratch = tempfile::tempdir().map_err(host_failure)?;
        let secrets = tempfile::tempdir().map_err(host_failure)?;
        for (name, value) in &environment.secrets {
            if name.is_empty()
                || name.contains('/')
                || name.contains('\\')
                || name == "."
                || name == ".."
            {
                return Err(host_failure(format!("invalid secret name {name:?}")));
            }
            std::fs::write(secrets.path().join(name), value).map_err(host_failure)?;
        }
        let mut wasi = WasiCtxBuilder::new();
        if environment.scratch {
            wasi.preopened_dir(scratch.path(), "/scratch", FsPerms::ReadWrite)
                .map_err(host_failure)?;
        }
        wasi.preopened_dir(secrets.path(), "/secrets", FsPerms::ReadOnly)
            .map_err(host_failure)?;
        if let Some(workspace) = &environment.workspace {
            wasi.preopened_dir(workspace, "/workspace", FsPerms::ReadWrite)
                .map_err(host_failure)?;
        }
        let mut http = WasiHttpCtx::new();
        http.set_field_size_limit(64 * 1024);
        Ok(Self {
            limits,
            bridge: Some(bridge),
            table: ResourceTable::new(),
            wasi: wasi.build(),
            http,
            network: NetworkHooks {
                allow: environment.network_allow,
            },
            _scratch: scratch,
            _secrets: secrets,
        })
    }

    async fn call(&mut self, call: HostCall) -> Result<String, TurnError> {
        let Some(bridge) = &self.bridge else {
            return Err(TurnError::new(
                "no_turn",
                "the guest called the host outside a turn",
            ));
        };
        bridge.call(call).await
    }
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl WasiHttpView for HostState {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            ctx: &mut self.http,
            table: &mut self.table,
            hooks: &mut self.network,
        }
    }
}

struct NetworkHooks {
    allow: Vec<String>,
}

impl WasiHttpHooks for NetworkHooks {
    fn send_request(
        &mut self,
        request: http::Request<WasiBody>,
        options: Option<RequestOptions>,
        _io: Box<dyn Future<Output = Result<(), HttpError>> + Send>,
    ) -> Box<
        dyn Future<
                Output = Result<
                    (
                        http::Response<WasiBody>,
                        Box<dyn Future<Output = Result<(), HttpError>> + Send>,
                    ),
                    HttpError,
                >,
            > + Send,
    > {
        let allowed = network_allowed(&self.allow, request.uri());
        Box::new(async move {
            if !allowed {
                return Err(HttpError::HttpRequestDenied);
            }
            let (response, io) = wasmtime_wasi_http::default_send_request(
                request,
                Some(bounded_http_options(options)),
            )
            .await?;
            Ok((
                response.map(|body| body.boxed_unsync()),
                Box::new(io) as Box<dyn Future<Output = Result<(), HttpError>> + Send>,
            ))
        })
    }
}

fn network_allowed(allow: &[String], uri: &http::Uri) -> bool {
    uri.scheme_str()
        .zip(uri.authority().map(|value| value.as_str()))
        .is_some_and(|(scheme, authority)| {
            let origin = format!("{scheme}://{authority}");
            allow.iter().any(|entry| {
                entry.eq_ignore_ascii_case(authority)
                    || entry.trim_end_matches('/').eq_ignore_ascii_case(&origin)
            })
        })
}

fn bounded_http_options(options: Option<RequestOptions>) -> RequestOptions {
    const MAX: Duration = Duration::from_secs(120);
    let options = options.unwrap_or_default();
    RequestOptions {
        connect_timeout: Some(options.connect_timeout.unwrap_or(MAX).min(MAX)),
        first_byte_timeout: Some(options.first_byte_timeout.unwrap_or(MAX).min(MAX)),
        between_bytes_timeout: Some(options.between_bytes_timeout.unwrap_or(MAX).min(MAX)),
    }
}

impl bindings::brain::agentloop::host::Host for HostState {
    async fn model(&mut self, request_json: String) -> Result<String, wit::TurnError> {
        self.call(HostCall::Model { request_json })
            .await
            .map_err(wit_error)
    }

    async fn dispatch(&mut self, calls_json: String) -> Result<String, wit::TurnError> {
        self.call(HostCall::Dispatch { calls_json })
            .await
            .map_err(wit_error)
    }

    async fn emit(&mut self, kind: String, payload_json: String) -> Result<u64, wit::TurnError> {
        let answer = self
            .call(HostCall::Emit { kind, payload_json })
            .await
            .map_err(wit_error)?;
        answer.trim().parse().map_err(|_| {
            wit_error(TurnError::new(
                "internal",
                "emit answered without a sequence",
            ))
        })
    }

    async fn telemetry(&mut self, record_json: String) {
        let _ = self.call(HostCall::Telemetry { record_json }).await;
    }
}

impl tool_bindings::brain::tool::host::Host for HostState {
    async fn emit(
        &mut self,
        kind: String,
        payload_json: String,
    ) -> Result<u64, tool_bindings::brain::tool::types::ToolError> {
        let answer = self
            .call(HostCall::Emit { kind, payload_json })
            .await
            .map_err(|error| tool_bindings::brain::tool::types::ToolError {
                code: error.code,
                message: error.message,
            })?;
        answer
            .trim()
            .parse()
            .map_err(|_| tool_bindings::brain::tool::types::ToolError {
                code: "internal".into(),
                message: "emit answered without a sequence".into(),
            })
    }

    async fn telemetry(&mut self, record_json: String) {
        let _ = self.call(HostCall::Telemetry { record_json }).await;
    }
}

fn wit_error(error: TurnError) -> wit::TurnError {
    wit::TurnError {
        code: error.code,
        message: error.message,
        retryable: error.retryable,
    }
}

/// Core instances one guest may hold: its own modules plus the shims wasmtime builds
/// for its imports.
const MAX_CORE_INSTANCES: usize = 8;

impl AdmittedAgentloop {
    /// Runs one turn in a fresh Store and Component instance. Only compiled code is
    /// retained between invocations.
    pub async fn turn(
        &self,
        engine: &Engine,
        limits: &LoopLimits,
        environment: NativeEnvironment,
        input: TurnInput,
        bridge: Arc<dyn GuestHost>,
    ) -> Result<TurnOutput, TurnError> {
        let store_limits = StoreLimitsBuilder::new()
            .memory_size(limits.linear_memory_bytes)
            .instances(MAX_CORE_INSTANCES)
            .build();
        let state = HostState::new(store_limits, bridge, environment)?;
        let mut store = Store::new(engine, state);
        store.limiter(|state| &mut state.limits);
        configure_fuel(&mut store, limits)?;
        let mut linker = Linker::<HostState>::new(engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(host_failure)?;
        wasmtime_wasi_http::p2::add_only_http_to_linker_async(&mut linker).map_err(host_failure)?;
        bindings::brain::agentloop::host::add_to_linker::<_, HasSelf<HostState>>(
            &mut linker,
            |state| state,
        )
        .map_err(host_failure)?;
        let bindings = bindings::Agentloop::instantiate_async(&mut store, &self.component, &linker)
            .await
            .map_err(host_failure)?;
        let input = to_wit_input(input).map_err(host_failure)?;
        let called = bindings.call_turn(&mut store, &input).await;
        let output = match called {
            Ok(Ok(output)) => output,
            // The loop's own failure, with the code it chose.
            Ok(Err(error)) => {
                return Err(TurnError {
                    code: error.code,
                    message: error.message,
                    retryable: error.retryable,
                });
            }
            // A trapped guest's heap is not a state anyone can vouch for: the entry is
            // dropped rather than kept.
            Err(error) => return Err(host_failure(turn_error(error))),
        };
        let output = from_wit_output(output).map_err(host_failure)?;
        validate_output(&output).map_err(host_failure)?;
        Ok(output)
    }
}

impl AdmittedTool {
    pub async fn run(
        &self,
        engine: &Engine,
        limits: &LoopLimits,
        environment: NativeEnvironment,
        input: NativeToolInput,
        bridge: Arc<dyn GuestHost>,
    ) -> Result<serde_json::Value, TurnError> {
        let store_limits = StoreLimitsBuilder::new()
            .memory_size(limits.linear_memory_bytes)
            .instances(MAX_CORE_INSTANCES)
            .build();
        let state = HostState::new(store_limits, bridge, environment)?;
        let mut store = Store::new(engine, state);
        store.limiter(|state| &mut state.limits);
        configure_fuel(&mut store, limits)?;
        let mut linker = Linker::<HostState>::new(engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(host_failure)?;
        wasmtime_wasi_http::p2::add_only_http_to_linker_async(&mut linker).map_err(host_failure)?;
        tool_bindings::brain::tool::host::add_to_linker::<_, HasSelf<HostState>>(
            &mut linker,
            |state| state,
        )
        .map_err(host_failure)?;
        let bindings = tool_bindings::Tool::instantiate_async(&mut store, &self.component, &linker)
            .await
            .map_err(host_failure)?;
        let input = tool_bindings::brain::tool::types::Invocation {
            call_id: input.call_id,
            input_json: serde_json::to_string(&input.input).map_err(host_failure)?,
            configuration_json: serde_json::to_string(&input.configuration)
                .map_err(host_failure)?,
            deadline_at_ms: input.deadline_at_ms,
        };
        match bindings.call_run(&mut store, &input).await {
            Ok(Ok(output)) => serde_json::from_str(&output)
                .map_err(|error| host_failure(format!("Tool output is invalid JSON: {error}"))),
            Ok(Err(error)) => Err(TurnError::new(error.code, error.message)),
            Err(error) => Err(host_failure(turn_error(error))),
        }
    }
}

const COMPUTE_BUDGET_EXCEEDED: &str = "native invocation exceeded its compute budget";

fn configure_fuel(store: &mut Store<HostState>, limits: &LoopLimits) -> Result<(), TurnError> {
    store.set_fuel(limits.fuel).map_err(host_failure)?;
    store
        .fuel_async_yield_interval(Some(FUEL_YIELD_INTERVAL.min(limits.fuel).max(1)))
        .map_err(host_failure)
}

/// A turn that failed on this side of the guest: a trap, a budget, an output the
/// contract refuses. The loop did not choose a code, so it gets the one that says so.
fn host_failure(message: impl std::fmt::Display) -> TurnError {
    TurnError::new(codes::failure::AGENTLOOP_FAILED, message.to_string())
}

/// A guest stopped by fuel exhaustion reports the budget it exceeded, not Wasmtime's trap.
fn turn_error(error: wasmtime::Error) -> String {
    if error.downcast_ref::<Trap>() == Some(&Trap::OutOfFuel) {
        return COMPUTE_BUDGET_EXCEEDED.into();
    }
    error.to_string()
}

fn to_wit_input(input: TurnInput) -> Result<wit::TurnInput, String> {
    let json = |value: &dyn erased_serialize::Serialize| -> Result<String, String> {
        value.to_json().map_err(|error| error.to_string())
    };
    Ok(wit::TurnInput {
        input_json: json(&input.input)?,
        transcript_json: json(&input.transcript)?,
        slots_json: json(&input.slots)?,
        events_json: json(&input.events)?,
        configuration_json: json(&input.configuration)?,
        system: input.system,
        tools_json: json(&input.tools)?,
        runtime: wit::RuntimeEnvelope {
            logical_time_ms: input.runtime.logical_time_ms,
            deterministic_seed: input.runtime.deterministic_seed,
        },
    })
}

/// A tiny shim so `to_wit_input` can serialise fields of different types through one
/// closure without a generic bound per call.
mod erased_serialize {
    pub trait Serialize {
        fn to_json(&self) -> Result<String, serde_json::Error>;
    }
    impl<T: serde::Serialize> Serialize for T {
        fn to_json(&self) -> Result<String, serde_json::Error> {
            serde_json::to_string(self)
        }
    }
}

fn from_wit_output(output: wit::TurnOutput) -> Result<TurnOutput, String> {
    Ok(TurnOutput {
        transcript: serde_json::from_str(&output.transcript_json)
            .map_err(|error| format!("Agentloop transcript is invalid JSON: {error}"))?,
        slots: serde_json::from_str(&output.slots_json)
            .map_err(|error| format!("Agentloop slots are invalid JSON: {error}"))?,
        result: output
            .result_json
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|error| format!("Agentloop result is invalid JSON: {error}"))?,
    })
}

fn validate_output(output: &TurnOutput) -> Result<(), String> {
    if output.transcript.len() > brain_protocol::MAX_TRANSCRIPT_ITEMS {
        return Err(format!(
            "Agentloop transcript exceeds {} items",
            brain_protocol::MAX_TRANSCRIPT_ITEMS
        ));
    }
    if output.slots.len() > 128 || output.slots.keys().any(|name| !valid_identifier(name)) {
        return Err("Agentloop slots must be at most 128 identifier-named values".into());
    }
    Ok(())
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'.' | b'_' | b':' | b'-'))
        })
}

fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use wasm_encoder::{
        BlockType, CodeSection, ExportKind, ExportSection, Function, FunctionSection, Instruction,
        Module, TypeSection, ValType,
    };
    use wasmtime::{Instance, Module as CoreModule, Store, Trap};

    use super::*;

    const TEST_FUEL: u64 = 10_000;
    const CEILING: Duration = Duration::from_secs(10);
    const HOST_WAIT: Duration = Duration::from_millis(250);

    struct SlowHost;

    #[async_trait::async_trait]
    impl GuestHost for SlowHost {
        async fn call(&self, _call: HostCall) -> Result<String, TurnError> {
            tokio::time::sleep(HOST_WAIT).await;
            Ok(String::new())
        }
    }

    fn empty_environment() -> NativeEnvironment {
        NativeEnvironment {
            scratch: false,
            workspace: None,
            network_allow: Vec::new(),
            secrets: Default::default(),
        }
    }

    #[test]
    fn native_network_is_default_deny_and_matches_only_an_authority_or_origin() {
        let https = "https://api.example.com/path".parse().unwrap();
        let http = "http://api.example.com/path".parse().unwrap();
        assert!(!network_allowed(&[], &https));
        assert!(network_allowed(&["api.example.com".into()], &https));
        assert!(network_allowed(&["api.example.com".into()], &http));
        assert!(network_allowed(&["https://api.example.com".into()], &https));
        assert!(!network_allowed(&["https://api.example.com".into()], &http));
        assert!(!network_allowed(&["example.com".into()], &https));
    }

    /// `(func (export "spin") (loop (br 0)))` - a backedge and nothing else, so the only
    /// way out is a trap at the loop's interruption check.
    fn spinning_module() -> Vec<u8> {
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types
            .ty()
            .function(Vec::<ValType>::new(), Vec::<ValType>::new());
        module.section(&types);

        let mut functions = FunctionSection::new();
        functions.function(0);
        module.section(&functions);

        let mut exports = ExportSection::new();
        exports.export("spin", ExportKind::Func, 0);
        module.section(&exports);

        let mut code = CodeSection::new();
        let mut spin = Function::new([]);
        spin.instruction(&Instruction::Loop(BlockType::Empty));
        spin.instruction(&Instruction::Br(0));
        spin.instruction(&Instruction::End);
        spin.instruction(&Instruction::End);
        code.function(&spin);
        module.section(&code);

        module.finish()
    }

    /// The smallest module that never returns consumes its fixed work allowance.
    #[tokio::test]
    async fn a_guest_that_never_returns_is_trapped_within_its_budget() {
        let limits = LoopLimits::default();
        let fuel = limits.fuel;
        let admission = AdmissionEngine::new(limits, Vec::new()).unwrap();
        let engine = admission.engine();

        let module = CoreModule::new(engine, spinning_module()).unwrap();
        let mut store = Store::new(engine, ());
        store.set_fuel(fuel).unwrap();
        store
            .fuel_async_yield_interval(Some(FUEL_YIELD_INTERVAL))
            .unwrap();
        let instance = Instance::new_async(&mut store, &module, &[]).await.unwrap();
        let spin = instance
            .get_typed_func::<(), ()>(&mut store, "spin")
            .unwrap();

        let started = Instant::now();
        let outcome = tokio::time::timeout(CEILING, spin.call_async(&mut store, ()))
            .await
            .unwrap_or_else(|_| {
                panic!("the guest was still running after {CEILING:?} on its fuel budget")
            });
        let elapsed = started.elapsed();

        let error = outcome.expect_err("a guest that never returns must not return");
        assert_eq!(
            error.downcast_ref::<Trap>(),
            Some(&Trap::OutOfFuel),
            "the guest must be stopped by fuel exhaustion, not by anything else: {error}"
        );
        assert_eq!(turn_error(error), COMPUTE_BUDGET_EXCEEDED);
        assert!(
            elapsed < CEILING,
            "the guest ran for {elapsed:?} after exhausting its fuel"
        );
    }

    #[tokio::test]
    async fn a_slow_async_host_wait_consumes_no_guest_fuel() {
        let admission = AdmissionEngine::new(LoopLimits::default(), Vec::new()).unwrap();
        let engine = admission.engine();
        let state = HostState::new(
            StoreLimitsBuilder::new().build(),
            Arc::new(SlowHost),
            empty_environment(),
        )
        .unwrap();
        let mut store = Store::new(engine, state);
        configure_fuel(
            &mut store,
            &LoopLimits {
                fuel: TEST_FUEL,
                ..LoopLimits::default()
            },
        )
        .unwrap();
        let started = Instant::now();
        store
            .data_mut()
            .call(HostCall::Telemetry {
                record_json: "{}".into(),
            })
            .await
            .unwrap();
        assert!(started.elapsed() >= HOST_WAIT);
        assert_eq!(store.get_fuel().unwrap(), TEST_FUEL);
    }
}
