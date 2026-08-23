//! Explicit local-development implementation of Brain's typed Environment ports.
//!
//! This is the zero-setup default mode. Be clear about what it is NOT: a subprocess is
//! process-level separation, **not a sandbox** -- `bash` here runs arbitrary commands as the
//! operator. Fine for developing against your own brain with your own key; untrusted prompts
//! belong in a sandboxed Environment adapter. The startup banner repeats this.
//!
//! Semantics mirror the typed Environment contract where it matters to the model: digest-sealed Node 22
//! bundles, durable operation receipts/terminal acknowledgements, bounded output, and
//! cancellation with process-tree termination. Guest paths map onto the workspace: `/workspace/...` and
//! `/home/agent/...` both resolve into the session's directory; other absolute paths are
//! refused (hygiene, not security).

use async_trait::async_trait;
use base64::Engine as _;
use brain::adapter::CallOutcome;
use brain::environment::{
    EnvironmentPort, EnvironmentResult, SandboxControlPort, SandboxFileContent, SandboxFileList,
    SandboxFileListRequest, SandboxFilesPort, SandboxSearchRequest, SecretDeliveryPort,
    SessionPreparationPort,
};
use brain::{BrainError, Result};
use brain_protocol::environment::{
    AcknowledgeTerminalRequest, Acknowledgement, CancelRequest, CancellationReceipt,
    CreateSandboxRequest, EnvironmentError, EnvironmentErrorCode, ObserveRequest, OperationObservation,
    OperationRef, PrepareSessionRequest, PreparedSession, ResolvedBinding, SandboxCopyRequest,
    SandboxCopyRequestDirection, SandboxCopyResult, SandboxExecutionRequest, SandboxFileRequest,
    SandboxFileWriteRequest, SandboxFileWriteResult, SandboxFileWriteSource, SandboxStatus,
    SandboxTarget, SealedBinding, SubmitReceipt, SubmitRequest, TargetReceipt,
    TerminalOutcome as EnvironmentTerminalOutcome, TerminalResult, WriteStdinReceipt, WriteStdinRequest,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::Digest as _;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

/// Per-stream capture bound: tail-retained beyond this, like the guest's spill.
const MAX_STREAM_BYTES: usize = 1024 * 1024;
const MAX_MANAGED_OUTPUT_BYTES: usize = brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES;
const MAX_LIVE_SANDBOX_EXECUTIONS: usize = 256;
const LOCAL_TOOL_RUNNER: &str = include_str!("local_tool_runner.mjs");

#[derive(Debug, Clone)]
struct LocalManagedSpec {
    name: String,
    description: Option<String>,
    contract_digest: String,
    bundle_digest: String,
    required_env: Vec<String>,
}

/// Local mode deliberately is not a security boundary, but a child still must not inherit the
/// Brain/control-plane environment accidentally. Start from a tiny non-secret launch set; each
/// Tool then receives only the session values authorized by its sealed binding. PATH is retained
/// so the explicit local developer composition can find Node/bash and their normal subprocesses.
fn configure_minimal_process_env(command: &mut tokio::process::Command) {
    command.env_clear();
    const LAUNCH_ENV: &[&str] = &[
        "PATH",
        "PATHEXT",
        "SystemRoot",
        "WINDIR",
        "COMSPEC",
        "TEMP",
        "TMP",
        "TMPDIR",
        "LANG",
        "LC_ALL",
    ];
    for name in LAUNCH_ENV {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

/// One session's local workspace.
struct LocalWorkspace {
    pub session_id: String,
    root: PathBuf,
    bundles: PathBuf,
    runtime: PathBuf,
    managed_tools: Arc<HashMap<String, LocalManagedSpec>>,
    env: Arc<HashMap<String, String>>,
}

#[cfg(test)]
mod typed_local_tests {
    use super::*;
    use brain_protocol::environment::{OperationEnvelope, OperationState};

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "brain-local-environment-{name}-{}",
            brain::mint_id("t", 12)
        ))
    }

    fn bundle(dir: &Path) -> (Vec<u8>, String, String) {
        let contract = "1".repeat(64);
        let source = format!(
            "export default {{kind:'brain.tool-runtime',name:'echo',description:null,contractDigest:'{contract}',requiredEnv:[],execute:async(input,context)=>({{...input,operation:context.operationId}})}};"
        );
        let bytes = source.into_bytes();
        let digest = hex::encode(sha2::Sha256::digest(&bytes));
        let path = dir.join("source.mjs");
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let url = url::Url::from_file_path(std::fs::canonicalize(path).unwrap())
            .unwrap()
            .to_string();
        (bytes, digest, url)
    }

    fn binding(digest: &str, bytes: usize) -> SealedBinding {
        typed(json!({
            "binding_id": "bnd_echo",
            "bundle": {
                "bundle_digest": digest,
                "bytes": bytes,
                "target": "linux-amd64",
                "execute_path": format!("/artifacts/{digest}/execute"),
                "setup_path": null,
                "object": {
                    "object_id": format!("bundle_{digest}"),
                    "bytes": bytes,
                    "sha256": digest,
                    "media_type": "application/javascript+esm",
                },
                "tool_name": "echo",
                "environment_name": "workspace",
                "description": null,
                "contract_digest": "1".repeat(64),
                "required_env": [],
            },
            "session_id": "ses_local",
            "root_id": "ses_local",
            "contract_digest": "1".repeat(64),
            "implementation_identity": digest,
            "environment_name": "workspace",
            "capability": "echo",
            "policy_digest": "2".repeat(64),
            "required_capabilities": ["execution", "session_preparation"],
        }))
        .unwrap()
    }

    async fn prepare(environment: &LocalEnvironment, binding: SealedBinding, url: &str) {
        environment.resolve_binding(binding.clone()).await.unwrap();
        let descriptor = binding.bundle.as_ref().unwrap();
        let request = typed(json!({
            "session_id": "ses_local",
            "root_id": "ses_local",
            "bindings": [{
                "binding_ref": "bnd_echo",
                "bundle_digests": [descriptor.bundle_digest],
            }],
            "bundles": [{
                "bundle_digest": descriptor.bundle_digest,
                "url": url,
                "headers": {},
                "expires_at_ms": brain::wall_ms() + 60_000,
                "max_bytes": descriptor.bytes,
            }],
            "network": {"kind":"none"},
            "resources": {
                "timeout_ms": 60_000,
                "max_output_bytes": brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
            },
        }))
        .unwrap();
        environment.prepare(request).await.unwrap();
    }

    fn submit_request(operation_id: &str) -> SubmitRequest {
        let mut envelope: OperationEnvelope = typed(json!({
            "operation_id": operation_id,
            "request_digest": "0".repeat(64),
            "session_id": "ses_local",
            "root_id": "ses_local",
            "turn_id": "trn_local",
            "caller_id": "agent_root",
            "fence": 1,
            "binding_ref": "bnd_echo",
            "capability": "echo",
            "input": {"kind":"inline","value":{"answer":42}},
            "phase": "execute",
            "deadline_at_ms": brain::wall_ms() + 60_000,
            "resources": {
                "timeout_ms": 60_000,
                "max_output_bytes": brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
            },
            "network": {"kind":"none"},
            "trace": {},
        }))
        .unwrap();
        envelope.request_digest = brain_protocol::contract::operation_request_digest(&envelope);
        SubmitRequest {
            envelope,
            wait_up_to_ms: 30_000,
        }
    }

    async fn materialize_test_default(environment: &LocalEnvironment) -> (SandboxTarget, String) {
        let target: SandboxTarget = typed(json!({
            "kind": "default",
            "session_id": "ses_local",
            "root_id": "ses_local",
            "binding_ref": "bnd_default",
            "sandbox_id": null,
        }))
        .unwrap();
        let generation = "gen_files".to_owned();
        let request: CreateSandboxRequest = typed(json!({
            "target": target,
            "generation_intent": generation,
            "network": {"kind":"none"},
            "resource_class": "microvm-1gb",
            "resources": {
                "timeout_ms": 60_000,
                "max_output_bytes": brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
            },
        }))
        .unwrap();
        let status = environment.materialize_default(request).await.unwrap();
        (status.target, status.generation.unwrap().to_string())
    }

    fn file_write_request(
        operation_id: &str,
        target: &SandboxTarget,
        generation: &str,
        text: &str,
    ) -> SandboxFileWriteRequest {
        let mut request: SandboxFileWriteRequest = typed(json!({
            "operation_id": operation_id,
            "request_digest": "0".repeat(64),
            "target": target,
            "expected_generation": generation,
            "path": "/workspace/result.txt",
            "source": {
                "kind": "inline",
                "content_base64": base64::engine::general_purpose::STANDARD.encode(text),
            },
            "overwrite": false,
        }))
        .unwrap();
        request.request_digest =
            brain_protocol::contract::sandbox_file_write_request_digest(&request);
        request
    }

    fn bash_available() -> bool {
        std::process::Command::new("bash")
            .arg("--version")
            .output()
            .is_ok()
    }

    fn sandbox_execution_request(
        target: &SandboxTarget,
        generation: &str,
        execution_id: &str,
        command: &str,
        interactive: bool,
    ) -> SandboxExecutionRequest {
        let mut request: SandboxExecutionRequest = typed(json!({
            "target": target,
            "expected_generation": generation,
            "execution_id": execution_id,
            "request_digest": "0".repeat(64),
            "input": {
                "command": command,
                "cwd": "/workspace",
                "interactive": interactive,
            },
            "resources": {
                "timeout_ms": 60_000,
                "max_output_bytes": brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
            },
            "network": {"kind":"none"},
        }))
        .unwrap();
        request.request_digest =
            brain_protocol::contract::sandbox_execution_request_digest(&request);
        request
    }

    fn stdin_request(
        target: &SandboxTarget,
        generation: &str,
        execution_id: &str,
        operation_id: &str,
        text: &str,
        eof: bool,
    ) -> WriteStdinRequest {
        let mut request: WriteStdinRequest = typed(json!({
            "operation_id": operation_id,
            "request_digest": "0".repeat(64),
            "target": target,
            "expected_generation": generation,
            "execution_id": execution_id,
            "text": text,
            "eof": eof,
        }))
        .unwrap();
        request.request_digest = brain_protocol::contract::write_stdin_request_digest(&request);
        request
    }

    #[tokio::test]
    async fn additional_sandbox_control_round_trips_create_exec_stdin_and_terminate() {
        if !bash_available() {
            return;
        }
        let dir = test_dir("additional-control");
        let environment = LocalEnvironment::open(dir.join("environment")).unwrap();
        let target: SandboxTarget = typed(json!({
            "kind": "additional",
            "session_id": "ses_local",
            "root_id": "ses_local",
            "binding_ref": "binding_sandbox",
            "sandbox_id": "sandbox_local",
        }))
        .unwrap();
        let create: CreateSandboxRequest = typed(json!({
            "target": target,
            "generation_intent": "generation_local",
            "network": {"kind":"none"},
            "resource_class": "microvm-1gb",
            "resources": {
                "timeout_ms": 60_000,
                "max_output_bytes": brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
            },
        }))
        .unwrap();
        let created = SandboxControlPort::create(&*environment, create).await.unwrap();
        assert_eq!(created.state, brain_protocol::environment::SandboxState::Running);
        let generation = created.generation.as_ref().unwrap().to_string();
        let inspected = SandboxControlPort::inspect(&*environment, target.clone())
            .await
            .unwrap();
        assert_eq!(inspected.target_ref, created.target_ref);

        let completed = SandboxControlPort::execute(
            &*environment,
            sandbox_execution_request(
                &target,
                &generation,
                "execution_file",
                "printf local-control > control.txt; printf completed",
                false,
            ),
        )
        .await
        .unwrap();
        assert_eq!(completed.observation.state, OperationState::Terminal);
        assert_eq!(
            completed.observation.terminal.as_ref().unwrap().outcome,
            EnvironmentTerminalOutcome::Completed
        );
        let content = SandboxFilesPort::read(
            &*environment,
            typed(json!({
                "target": target,
                "expected_generation": generation,
                "path": "/workspace/control.txt",
            }))
            .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(content.content_base64)
                .unwrap(),
            b"local-control"
        );

        let interactive = SandboxControlPort::execute(
            &*environment,
            sandbox_execution_request(
                &target,
                &generation,
                "execution_stdin",
                "IFS= read -r line; printf 'got:%s' \"$line\"",
                true,
            ),
        )
        .await
        .unwrap();
        assert_eq!(interactive.observation.state, OperationState::Running);
        let accepted = SandboxControlPort::write_stdin(
            &*environment,
            stdin_request(
                &target,
                &generation,
                "execution_stdin",
                "stdin_append",
                "hello\n",
                true,
            ),
        )
        .await
        .unwrap();
        assert!(accepted.accepted);
        let poll = stdin_request(
            &target,
            &generation,
            "execution_stdin",
            "stdin_poll",
            "",
            false,
        );
        let terminal = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let receipt = SandboxControlPort::write_stdin(&*environment, poll.clone())
                    .await
                    .unwrap();
                if receipt.observation.state == OperationState::Terminal {
                    break receipt;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("interactive local sandbox reaches a terminal");
        assert!(
            terminal.replayed,
            "the exact poll refreshes its observation"
        );
        assert!(
            terminal
                .observation
                .output
                .iter()
                .any(|chunk| chunk.text.as_str().contains("got:hello"))
        );

        let terminated = SandboxControlPort::terminate(&*environment, target.clone())
            .await
            .unwrap();
        assert_eq!(
            terminated.state,
            brain_protocol::environment::SandboxState::Terminated
        );
        assert_eq!(
            SandboxControlPort::inspect(&*environment, target)
                .await
                .unwrap()
                .state,
            brain_protocol::environment::SandboxState::Terminated
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn sandbox_file_write_lost_response_replays_without_repeating_effect() {
        let dir = test_dir("file-write-replay");
        let (bytes, digest, url) = bundle(&dir);
        let sealed = binding(&digest, bytes.len());
        let environment = LocalEnvironment::open(dir.join("environment")).unwrap();
        prepare(&environment, sealed.clone(), &url).await;
        let (target, generation) = materialize_test_default(&environment).await;
        let request = file_write_request("file-op-1", &target, &generation, "first");

        let first = environment.write(request.clone()).await.unwrap();
        assert!(!first.replayed);
        drop(environment);

        let restarted = LocalEnvironment::open(dir.join("environment")).unwrap();
        prepare(&restarted, sealed, &url).await;
        let replay = restarted.write(request).await.unwrap();
        assert!(replay.replayed);
        assert_eq!(
            serde_jcs::to_vec(&first.file).unwrap(),
            serde_jcs::to_vec(&replay.file).unwrap()
        );

        let conflict = restarted
            .write(file_write_request(
                "file-op-1",
                &target,
                &generation,
                "different",
            ))
            .await
            .unwrap_err();
        assert_eq!(conflict.code, EnvironmentErrorCode::BindingConflict);
        let read: SandboxFileRequest = typed(json!({
            "target": target,
            "expected_generation": generation,
            "path": "/workspace/result.txt",
        }))
        .unwrap();
        let content = restarted.read(read).await.unwrap();
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(content.content_base64)
                .unwrap(),
            b"first"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn terminal_replays_across_local_environment_restart_and_ack_is_idempotent() {
        let dir = test_dir("replay");
        let (bytes, digest, url) = bundle(&dir);
        let sealed = binding(&digest, bytes.len());
        let environment = LocalEnvironment::open(dir.join("environment")).unwrap();
        prepare(&environment, sealed.clone(), &url).await;

        let request = submit_request("op_replay");
        let first = environment.submit(request.clone()).await.unwrap();
        assert!(!first.replayed);
        assert_eq!(first.observation.state, OperationState::Terminal);
        assert_eq!(
            first
                .observation
                .terminal
                .as_ref()
                .unwrap()
                .inline
                .as_ref()
                .unwrap()["answer"],
            42
        );

        drop(environment);
        let restarted = LocalEnvironment::open(dir.join("environment")).unwrap();
        prepare(&restarted, sealed, &url).await;
        let replay = restarted.submit(request).await.unwrap();
        assert!(replay.replayed);
        assert_eq!(
            serde_jcs::to_vec(&first.observation.terminal).unwrap(),
            serde_jcs::to_vec(&replay.observation.terminal).unwrap()
        );

        let terminal_digest = replay
            .observation
            .terminal
            .as_ref()
            .unwrap()
            .terminal_digest
            .clone();
        let ack: AcknowledgeTerminalRequest = typed(json!({
            "operation": replay.operation,
            "terminal_digest": terminal_digest,
        }))
        .unwrap();
        assert!(
            restarted
                .acknowledge_terminal(ack.clone())
                .await
                .unwrap()
                .acknowledged
        );
        assert!(
            restarted
                .acknowledge_terminal(ack)
                .await
                .unwrap()
                .acknowledged
        );
        assert_eq!(
            restarted
                .submit(submit_request("op_replay"))
                .await
                .unwrap_err()
                .code,
            EnvironmentErrorCode::OperationUnknown
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn request_only_crash_is_interrupted_without_repeating_the_effect() {
        let dir = test_dir("unknown");
        let (bytes, digest, url) = bundle(&dir);
        let sealed = binding(&digest, bytes.len());
        let environment = LocalEnvironment::open(dir.join("environment")).unwrap();
        prepare(&environment, sealed.clone(), &url).await;
        let request = submit_request("op_unknown");
        let target: SandboxTarget = typed(json!({
            "kind":"default",
            "session_id":"ses_local",
            "root_id":"ses_local",
            "binding_ref":"bnd_echo",
        }))
        .unwrap();
        let (status, _) = environment.ensure_target(target.clone(), None).await.unwrap();
        let generation = status.generation.as_ref().unwrap();
        let target_ref = status.target_ref.as_ref().unwrap();
        let operation: OperationRef = typed(json!({
            "operation_id":"op_unknown",
            "request_digest":request.envelope.request_digest,
            "target":target,
            "generation":generation,
            "target_ref":target_ref,
            "receipt_ref":"receipt_crash",
        }))
        .unwrap();
        let target_receipt: TargetReceipt = typed(json!({
            "generation":generation,
            "target_ref":target_ref,
            "expires_at_ms":status.expires_at_ms,
        }))
        .unwrap();
        let durable = DurableOperation {
            envelope: request.envelope.clone(),
            operation,
            target: target_receipt,
        };
        write_new_json(
            &environment
                .operation_dir("ses_local", "op_unknown")
                .join("request.json"),
            &durable,
            "inject request-only crash",
        )
        .unwrap();
        drop(environment);

        let restarted = LocalEnvironment::open(dir.join("environment")).unwrap();
        prepare(&restarted, sealed, &url).await;
        let replay = restarted.submit(request).await.unwrap();
        assert!(replay.replayed);
        let terminal = replay.observation.terminal.unwrap();
        assert_eq!(terminal.outcome, EnvironmentTerminalOutcome::Interrupted);
        assert!(terminal.is_error);
        let _ = std::fs::remove_dir_all(dir);
    }
}

impl LocalWorkspace {
    /// Opens (creating if needed) the session's directories under `data_dir`.
    pub fn open(data_dir: &Path, session_id: &str) -> Result<Arc<Self>> {
        Self::open_with_runtime(data_dir, session_id, session_id, Vec::new(), HashMap::new())
    }

    fn open_with_runtime(
        data_dir: &Path,
        physical_workspace_id: &str,
        session_id: &str,
        managed_tools: Vec<LocalManagedSpec>,
        env: HashMap<String, String>,
    ) -> Result<Arc<Self>> {
        let base = data_dir.join(physical_workspace_id);
        let root = base.join("workspace");
        let bundles = base.join("bundles");
        let runtime = base.join("runtime");
        std::fs::create_dir_all(&root)
            .and_then(|()| std::fs::create_dir_all(&bundles))
            .and_then(|()| std::fs::create_dir_all(&runtime))
            .map_err(|e| BrainError::EnvironmentUnavailable(format!("workspace dir: {e}")))?;
        std::fs::write(runtime.join("tool-runner.mjs"), LOCAL_TOOL_RUNNER)
            .map_err(|e| BrainError::EnvironmentUnavailable(format!("local Tool runner: {e}")))?;
        Ok(Arc::new(Self {
            session_id: session_id.to_string(),
            root,
            bundles,
            runtime,
            managed_tools: Arc::new(
                managed_tools
                    .into_iter()
                    .map(|tool| (tool.name.clone(), tool))
                    .collect(),
            ),
            env: Arc::new(env),
        }))
    }

    /// Maps a guest-style path into the workspace. `/workspace/...` and `/home/agent/...`
    /// resolve here; bare relative paths resolve here; anything else absolute is refused, and
    /// `..` may never escape the root.
    fn resolve(&self, path: &str) -> Result<PathBuf> {
        let p = path.trim();
        let rel = if let Some(r) = p.strip_prefix("/workspace") {
            r.trim_start_matches('/')
        } else if let Some(r) = p.strip_prefix("/home/agent") {
            r.trim_start_matches('/')
        } else if Path::new(p).is_absolute() || p.starts_with('/') {
            return Err(BrainError::Invalid(format!(
                "path {p} is outside the workspace (use /workspace/... or a relative path)"
            )));
        } else {
            p
        };
        let mut out = self.root.clone();
        for c in Path::new(rel).components() {
            match c {
                Component::Normal(seg) => out.push(seg),
                Component::CurDir => {}
                _ => {
                    return Err(BrainError::Invalid(format!(
                        "path {p} escapes the workspace"
                    )));
                }
            }
        }
        Ok(out)
    }

    fn rel_of(&self, p: &Path) -> String {
        p.strip_prefix(&self.root)
            .unwrap_or(p)
            .to_string_lossy()
            .replace('\\', "/")
    }

    async fn execute_operation(
        self: &Arc<Self>,
        tool: &str,
        input: Value,
        operation_id: &str,
        deadline_at_ms: u64,
        cancel: &CancellationToken,
        emit: impl Fn(&str, u64, String) + Send + Sync + 'static,
    ) -> CallOutcome {
        let t0 = Instant::now();
        let Some(seal) = self.managed_tools.get(tool).cloned() else {
            return CallOutcome::failed(format!(
                "managed capability {tool} is outside the prepared binding set"
            ));
        };
        self.managed_tool(seal, input, operation_id, deadline_at_ms, cancel, emit, t0)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn managed_tool(
        &self,
        seal: LocalManagedSpec,
        input: Value,
        operation_id: &str,
        deadline_at_ms: u64,
        cancel: &CancellationToken,
        emit: impl Fn(&str, u64, String) + Send + Sync + 'static,
        t0: Instant,
    ) -> CallOutcome {
        let fail = |message: String, outcome: EnvironmentTerminalOutcome| CallOutcome {
            outcome,
            value: None,
            content: message,
            is_error: true,
            exit_code: None,
            duration_ms: t0.elapsed().as_millis() as u64,
            truncated: false,
            terminal: None,
        };
        for name in &seal.required_env {
            if !self.env.contains_key(name) {
                return fail(
                    format!("required environment variable {name} is unavailable"),
                    EnvironmentTerminalOutcome::Failed,
                );
            }
        }

        let run_id = brain::mint_id("run", 20);
        let request_path = self.runtime.join(format!("{run_id}.request.json"));
        let result_path = self.runtime.join(format!("{run_id}.result.json"));
        let bundle_path = self.bundles.join(format!("{}.mjs", seal.bundle_digest));
        let deadline_ms = deadline_at_ms;
        let timeout_ms = deadline_ms.saturating_sub(brain::wall_ms()).max(1);
        let request = json!({
            "operation_id": operation_id,
            "session_id": self.session_id,
            "seal": {
                "name": seal.name,
                "description": seal.description,
                "contract_digest": seal.contract_digest,
                "bundle_digest": seal.bundle_digest,
                "required_env": seal.required_env,
            },
            "input": input,
            "workspace": self.root.to_string_lossy(),
            "deadline_ms": deadline_ms,
            "max_output_bytes": MAX_MANAGED_OUTPUT_BYTES,
        });
        let request_bytes = match serde_json::to_vec(&request) {
            Ok(bytes) => bytes,
            Err(error) => {
                return fail(
                    format!("encode managed Tool request: {error}"),
                    EnvironmentTerminalOutcome::Failed,
                );
            }
        };
        if let Err(error) = tokio::fs::write(&request_path, request_bytes).await {
            return fail(
                format!("stage managed Tool request: {error}"),
                EnvironmentTerminalOutcome::Failed,
            );
        }

        let mut cmd = tokio::process::Command::new("node");
        configure_minimal_process_env(&mut cmd);
        cmd.arg(self.runtime.join("tool-runner.mjs"))
            .arg(&bundle_path)
            .arg(&request_path)
            .arg(&result_path)
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for name in &seal.required_env {
            if let Some(value) = self.env.get(name) {
                cmd.env(name, value);
            }
        }
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = tokio::fs::remove_file(&request_path).await;
                return fail(
                    format!("could not spawn Node 22 managed Tool runner: {error}"),
                    EnvironmentTerminalOutcome::Failed,
                );
            }
        };
        let mut stdout_pipe = child.stdout.take().expect("piped");
        let mut stderr_pipe = child.stderr.take().expect("piped");
        let emit = Arc::new(emit);
        let out_buf = Arc::new(std::sync::Mutex::new(StreamBuf::default()));
        let err_buf = Arc::new(std::sync::Mutex::new(StreamBuf::default()));
        let out_task = {
            let emit = emit.clone();
            let buf = out_buf.clone();
            tokio::spawn(
                async move { collect_stream(&mut stdout_pipe, "stdout", &*emit, &buf).await },
            )
        };
        let err_task = {
            let emit = emit.clone();
            let buf = err_buf.clone();
            tokio::spawn(
                async move { collect_stream(&mut stderr_pipe, "stderr", &*emit, &buf).await },
            )
        };

        let mut cancelled = false;
        let mut timed_out = false;
        let status = tokio::select! {
            status = child.wait() => status.ok(),
            () = cancel.cancelled() => {
                cancelled = true;
                kill_tree(&mut child).await;
                child.wait().await.ok()
            }
            () = tokio::time::sleep(std::time::Duration::from_millis(timeout_ms)) => {
                timed_out = true;
                kill_tree(&mut child).await;
                child.wait().await.ok()
            }
        };
        let drain = std::time::Duration::from_millis(1_500);
        let _ = tokio::time::timeout(drain, out_task).await;
        let _ = tokio::time::timeout(drain, err_task).await;
        let stdout = out_buf.lock().expect("stream buf").render();
        let stderr = err_buf.lock().expect("stream buf").render();
        let result = if cancelled || timed_out {
            None
        } else {
            match tokio::fs::metadata(&result_path).await {
                Ok(metadata) if metadata.len() <= MAX_MANAGED_OUTPUT_BYTES as u64 => {
                    tokio::fs::read(&result_path).await.ok().and_then(|bytes| {
                        (bytes.len() <= MAX_MANAGED_OUTPUT_BYTES)
                            .then(|| serde_json::from_slice::<Value>(&bytes).ok())
                            .flatten()
                    })
                }
                _ => None,
            }
        };
        let _ = tokio::fs::remove_file(&request_path).await;
        let _ = tokio::fs::remove_file(&result_path).await;
        let _ = tokio::fs::remove_file(result_path.with_extension("json.tmp")).await;

        if cancelled {
            return fail(
                "managed Tool call was cancelled".into(),
                EnvironmentTerminalOutcome::Cancelled,
            );
        }
        if timed_out {
            return fail(
                format!("managed Tool exceeded its {timeout_ms} ms remaining deadline"),
                EnvironmentTerminalOutcome::DeadlineExceeded,
            );
        }
        let exit_code = status.and_then(|status| status.code()).map(i64::from);
        match result {
            Some(result) if result.get("ok").and_then(Value::as_bool) == Some(true) => {
                let output = result.get("output").cloned().unwrap_or(Value::Null);
                let content = serde_json::to_string(&output).unwrap_or_else(|error| {
                    format!("managed Tool output encoding failed: {error}")
                });
                CallOutcome {
                    outcome: EnvironmentTerminalOutcome::Completed,
                    value: Some(output),
                    content,
                    is_error: exit_code != Some(0),
                    exit_code,
                    duration_ms: t0.elapsed().as_millis() as u64,
                    truncated: false,
                    terminal: None,
                }
            }
            Some(result) => {
                let message = result
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("managed Tool runner failed");
                let diagnostics = [stdout, stderr]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                let content = if diagnostics.is_empty() {
                    message.to_string()
                } else {
                    format!("{message}\n{diagnostics}")
                };
                CallOutcome {
                    outcome: EnvironmentTerminalOutcome::Failed,
                    value: None,
                    content,
                    is_error: true,
                    exit_code,
                    duration_ms: t0.elapsed().as_millis() as u64,
                    truncated: false,
                    terminal: None,
                }
            }
            None => fail(
                format!(
                    "managed Tool runner did not produce a valid bounded result{}",
                    if stderr.is_empty() {
                        String::new()
                    } else {
                        format!(": {stderr}")
                    }
                ),
                EnvironmentTerminalOutcome::Failed,
            ),
        }
    }
}

/// Kills a bash invocation AND its descendants. A bare `kill` orphans grandchildren, which
/// keep running and keep the output pipes open.
async fn kill_tree(child: &mut tokio::process::Child) {
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        let _ = tokio::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output()
            .await;
    }
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // The child was spawned as its own process group leader; negative pid = the group.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    let _ = child.kill().await;
}

/// A bounded, tail-retained stream capture (the end of the output is where compilers and
/// tests put the verdict).
struct StreamBuf {
    tail: String,
    elided: usize,
    limit: usize,
}

impl Default for StreamBuf {
    fn default() -> Self {
        Self {
            tail: String::new(),
            elided: 0,
            limit: MAX_STREAM_BYTES,
        }
    }
}

impl StreamBuf {
    fn bounded(limit: usize) -> Self {
        Self {
            limit: limit.max(1),
            ..Self::default()
        }
    }

    fn push(&mut self, text: &str) {
        self.tail.push_str(text);
        if self.tail.len() > self.limit {
            let cut = self.tail.len() - self.limit;
            let mut start = cut;
            while !self.tail.is_char_boundary(start) {
                start += 1;
            }
            self.elided += start;
            self.tail = self.tail.split_off(start);
        }
    }
    fn render(&self) -> String {
        if self.elided > 0 {
            format!(
                "[first {} bytes elided]
{}",
                self.elided, self.tail
            )
        } else {
            self.tail.clone()
        }
    }
}

/// Reads a pipe until EOF (or until abandoned), emitting chunks live into `emit` and the
/// shared buffer.
async fn collect_stream(
    pipe: &mut (impl tokio::io::AsyncRead + Unpin),
    name: &str,
    emit: &(impl Fn(&str, u64, String) + Send + Sync),
    into: &std::sync::Mutex<StreamBuf>,
) {
    let mut buf = [0u8; 8192];
    let mut offset = 0u64;
    loop {
        match pipe.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                emit(name, offset, text.clone());
                offset += n as u64;
                into.lock().expect("stream buf").push(&text);
            }
        }
    }
}

// ---- Canonical typed local Environment -----------------------------------------------------------

/// The durable local-development implementation of Brain's canonical Environment ports.
///
/// This is deliberately unsandboxed. It nevertheless preserves the same operation identity,
/// target-generation and terminal-acknowledgement ordering as a hosted Environment so restarting the
/// standalone Brain cannot silently execute an ambiguous Tool call twice.
pub struct LocalEnvironment {
    state_root: PathBuf,
    workspace_root: PathBuf,
    bindings: std::sync::RwLock<HashMap<String, SealedBinding>>,
    prepared: std::sync::RwLock<HashMap<String, PreparedRuntime>>,
    secret_delivery: std::sync::RwLock<Option<Arc<dyn SecretDeliveryPort>>>,
    operation_gates: std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    running: std::sync::Mutex<HashMap<String, CancellationToken>>,
    sandbox_executions: std::sync::Mutex<HashMap<String, Arc<LocalSandboxExecution>>>,
    sandbox_execution_slots: Arc<tokio::sync::Semaphore>,
    target_gate: tokio::sync::Mutex<()>,
}

#[derive(Clone)]
struct PreparedRuntime {
    root_id: String,
    binding_refs: std::collections::HashSet<String>,
    workspace: Arc<LocalWorkspace>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PhysicalTargetState {
    state: String,
    generation: String,
    target_ref: String,
    target_digest: String,
    changed_at_ms: u64,
    expires_at_ms: u64,
}

struct LocalSandboxExecution {
    operation: OperationRef,
    target: TargetReceipt,
    stdout: Arc<std::sync::Mutex<StreamBuf>>,
    stderr: Arc<std::sync::Mutex<StreamBuf>>,
    terminal: std::sync::RwLock<Option<TerminalResult>>,
    stdin: tokio::sync::Mutex<Option<tokio::process::ChildStdin>>,
    cancel: CancellationToken,
    completed: tokio::sync::Notify,
    _slot: tokio::sync::OwnedSemaphorePermit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DurableSandboxExecution {
    request: SandboxExecutionRequest,
    operation: OperationRef,
    target: TargetReceipt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DurableStdinEffect {
    request_digest: String,
    accepted: bool,
}

impl LocalSandboxExecution {
    fn observation(&self) -> EnvironmentResult<OperationObservation> {
        let stdout = self.stdout.lock().expect("sandbox stdout").render();
        let stderr = self.stderr.lock().expect("sandbox stderr").render();
        let terminal = self.terminal.read().expect("sandbox terminal").clone();
        sandbox_observation(
            self.operation.clone(),
            self.target.clone(),
            &stdout,
            &stderr,
            terminal,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DurableOperation {
    envelope: brain_protocol::environment::OperationEnvelope,
    operation: OperationRef,
    target: TargetReceipt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DurableFileEffectIdentity {
    kind: String,
    request_digest: String,
}

impl LocalEnvironment {
    pub fn open(data_dir: impl Into<PathBuf>) -> Result<Arc<Self>> {
        let root = data_dir.into();
        let state_root = root.join("state");
        let workspace_root = root.join("workspaces");
        std::fs::create_dir_all(&state_root)
            .and_then(|()| std::fs::create_dir_all(&workspace_root))
            .map_err(|error| BrainError::Journal(format!("create local Environment data: {error}")))?;
        Ok(Arc::new(Self {
            state_root,
            workspace_root,
            bindings: std::sync::RwLock::new(HashMap::new()),
            prepared: std::sync::RwLock::new(HashMap::new()),
            secret_delivery: std::sync::RwLock::new(None),
            operation_gates: std::sync::Mutex::new(HashMap::new()),
            running: std::sync::Mutex::new(HashMap::new()),
            sandbox_executions: std::sync::Mutex::new(HashMap::new()),
            sandbox_execution_slots: Arc::new(tokio::sync::Semaphore::new(
                MAX_LIVE_SANDBOX_EXECUTIONS,
            )),
            target_gate: tokio::sync::Mutex::new(()),
        }))
    }

    /// Complete the deliberate circular local composition after `Brain` has been constructed.
    /// The port is used only for one-purpose secret capabilities during preparation.
    pub fn attach_secret_delivery(&self, delivery: Arc<dyn SecretDeliveryPort>) -> EnvironmentResult<()> {
        let mut slot = self.secret_delivery.write().expect("local secret delivery");
        if let Some(existing) = slot.as_ref() {
            if Arc::ptr_eq(existing, &delivery) {
                return Ok(());
            }
            return Err(environment_error(
                EnvironmentErrorCode::BindingConflict,
                false,
                "local Environment secret delivery is already attached",
            ));
        }
        *slot = Some(delivery);
        Ok(())
    }

    fn root_dir(&self, root_id: &str) -> PathBuf {
        self.state_root.join(hash_component(root_id))
    }

    fn binding_path(&self, binding: &SealedBinding) -> PathBuf {
        self.root_dir(binding.root_id.as_str())
            .join("bindings")
            .join(format!(
                "{}.json",
                hash_component(binding.binding_id.as_str())
            ))
    }

    fn operation_dir(&self, root_id: &str, operation_id: &str) -> PathBuf {
        self.root_dir(root_id)
            .join("operations")
            .join(hash_component(operation_id))
    }

    fn file_effect_dir(&self, root_id: &str, operation_id: &str) -> PathBuf {
        self.root_dir(root_id)
            .join("file-effects")
            .join(hash_component(operation_id))
    }

    fn reserve_file_effect(
        &self,
        root_id: &str,
        operation_id: &str,
        kind: &str,
        request_digest: &str,
    ) -> EnvironmentResult<(PathBuf, bool)> {
        let dir = self.file_effect_dir(root_id, operation_id);
        let identity_path = dir.join("identity.json");
        if let Some(existing) = read_json_if_exists::<DurableFileEffectIdentity>(
            &identity_path,
            "sandbox file effect identity",
        )? {
            if existing.kind != kind || existing.request_digest != request_digest {
                return Err(environment_error(
                    EnvironmentErrorCode::BindingConflict,
                    false,
                    "sandbox file operation is already sealed to a different request",
                ));
            }
            return Ok((dir, true));
        }
        write_new_json(
            &identity_path,
            &DurableFileEffectIdentity {
                kind: kind.to_owned(),
                request_digest: request_digest.to_owned(),
            },
            "reserve sandbox file effect",
        )?;
        Ok((dir, false))
    }

    fn operation_gate(&self, root_id: &str, operation_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let key = format!(
            "{}:{}",
            hash_component(root_id),
            hash_component(operation_id)
        );
        self.operation_gates
            .lock()
            .expect("local operation gates")
            .entry(key)
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    fn binding(&self, binding_ref: &str) -> EnvironmentResult<SealedBinding> {
        self.bindings
            .read()
            .expect("local bindings")
            .get(binding_ref)
            .cloned()
            .ok_or_else(|| {
                environment_error(
                    EnvironmentErrorCode::CapabilityUnavailable,
                    false,
                    "managed binding must be resolved before preparation",
                )
            })
    }

    fn validate_target_shape(target: &SandboxTarget) -> EnvironmentResult<()> {
        let valid = match target.kind {
            brain_protocol::environment::TargetKind::Default => target.sandbox_id.is_none(),
            brain_protocol::environment::TargetKind::Additional => target.sandbox_id.is_some(),
        };
        if !valid {
            return Err(environment_error(
                EnvironmentErrorCode::InvalidRequest,
                false,
                "sandbox target kind and sandbox_id disagree",
            ));
        }
        Ok(())
    }

    fn target_digest(target: &SandboxTarget) -> String {
        hash_component(
            &String::from_utf8(
                serde_jcs::to_vec(target).expect("a generated sandbox target canonicalizes"),
            )
            .expect("canonical sandbox target JSON is UTF-8"),
        )
    }

    fn target_component(target: &SandboxTarget) -> String {
        match target.kind {
            brain_protocol::environment::TargetKind::Default => "default".into(),
            brain_protocol::environment::TargetKind::Additional => format!(
                "additional-{}",
                hash_component(
                    target
                        .sandbox_id
                        .as_ref()
                        .expect("validated additional target has a sandbox_id")
                        .as_str()
                )
            ),
        }
    }

    fn status_dir(&self, target: &SandboxTarget) -> PathBuf {
        let root = self.root_dir(target.root_id.as_str());
        match target.kind {
            // Preserve the durable-local default layout used before additional targets existed.
            brain_protocol::environment::TargetKind::Default => root.join("target-events"),
            brain_protocol::environment::TargetKind::Additional => root
                .join("additional-targets")
                .join(Self::target_component(target))
                .join("target-events"),
        }
    }

    fn target_workspace_id(target: &SandboxTarget) -> String {
        let root = hash_component(target.root_id.as_str());
        match target.kind {
            brain_protocol::environment::TargetKind::Default => root,
            brain_protocol::environment::TargetKind::Additional => {
                format!("{root}/additional/{}", Self::target_component(target))
            }
        }
    }

    fn read_physical_target(
        &self,
        target: &SandboxTarget,
    ) -> EnvironmentResult<Option<PhysicalTargetState>> {
        Self::validate_target_shape(target)?;
        let dir = self.status_dir(target);
        let mut paths = match std::fs::read_dir(&dir) {
            Ok(entries) => entries
                .filter_map(std::result::Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
                .collect::<Vec<_>>(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(local_io_error("read target state", error)),
        };
        paths.sort();
        let state = paths
            .last()
            .map(|path| read_json(path, "target state"))
            .transpose()?;
        if state.as_ref().is_some_and(|state: &PhysicalTargetState| {
            state.target_digest != Self::target_digest(target)
        }) {
            return Err(environment_error(
                EnvironmentErrorCode::BindingConflict,
                false,
                "sandbox target is sealed to a different root, owner, or binding",
            ));
        }
        Ok(state)
    }

    fn append_physical_target(
        &self,
        target: &SandboxTarget,
        state: &PhysicalTargetState,
    ) -> EnvironmentResult<()> {
        if state.target_digest != Self::target_digest(target) {
            return Err(environment_error(
                EnvironmentErrorCode::BindingConflict,
                false,
                "sandbox target state digest does not match its logical target",
            ));
        }
        let dir = self.status_dir(target);
        std::fs::create_dir_all(&dir)
            .map_err(|error| local_io_error("create target state", error))?;
        let next = std::fs::read_dir(&dir)
            .map_err(|error| local_io_error("read target state", error))?
            .filter_map(std::result::Result::ok)
            .count() as u64;
        write_new_json(
            &dir.join(format!("{next:020}.json")),
            state,
            "append target state",
        )
    }

    fn status_for(
        &self,
        target: SandboxTarget,
        physical: &PhysicalTargetState,
    ) -> EnvironmentResult<SandboxStatus> {
        typed(json!({
            "target": target,
            "state": physical.state,
            "target_ref": physical.target_ref,
            "generation": physical.generation,
            "changed_at_ms": physical.changed_at_ms,
            "expires_at_ms": physical.expires_at_ms,
        }))
    }

    async fn ensure_target(
        &self,
        target: SandboxTarget,
        generation_intent: Option<&str>,
    ) -> EnvironmentResult<(SandboxStatus, bool)> {
        Self::validate_target_shape(&target)?;
        let _guard = self.target_gate.lock().await;
        if let Some(current) = self.read_physical_target(&target)? {
            if matches!(current.state.as_str(), "gone" | "terminated") {
                return Err(environment_error(
                    EnvironmentErrorCode::SandboxGone,
                    false,
                    "the local default target has been terminated",
                ));
            }
            if current.expires_at_ms <= brain::wall_ms() {
                let gone = PhysicalTargetState {
                    state: "gone".into(),
                    changed_at_ms: brain::wall_ms(),
                    ..current
                };
                self.append_physical_target(&target, &gone)?;
                return Err(environment_error(
                    EnvironmentErrorCode::SandboxGone,
                    false,
                    "the local default target reached its hard expiry",
                ));
            }
            if let Some(expected) = generation_intent
                && current.generation != expected
            {
                return Err(environment_error(
                    EnvironmentErrorCode::GenerationConflict,
                    false,
                    "the local default target already has another generation",
                ));
            }
            return self
                .status_for(target, &current)
                .map(|status| (status, true));
        }
        let generation = generation_intent.map(str::to_owned).unwrap_or_else(|| {
            let digest = hash_component(&format!(
                "local-generation\0{}\0{}",
                target.root_id.as_str(),
                Self::target_component(&target)
            ));
            format!("gen_{}", &digest[..24])
        });
        let target_digest = hash_component(&format!(
            "local-target\0{}\0{}\0{generation}",
            target.root_id.as_str(),
            Self::target_component(&target)
        ));
        let physical = PhysicalTargetState {
            state: "running".into(),
            generation,
            target_ref: format!("tgt_{}", &target_digest[..24]),
            target_digest: Self::target_digest(&target),
            changed_at_ms: brain::wall_ms(),
            expires_at_ms: brain::wall_ms().saturating_add(8 * 60 * 60 * 1_000),
        };
        LocalWorkspace::open(&self.workspace_root, &Self::target_workspace_id(&target))
            .map_err(brain_error_to_environment)?;
        self.append_physical_target(&target, &physical)?;
        self.status_for(target, &physical)
            .map(|status| (status, false))
    }

    fn prepared_runtime(&self, session_id: &str) -> EnvironmentResult<PreparedRuntime> {
        self.prepared
            .read()
            .expect("local prepared sessions")
            .get(session_id)
            .cloned()
            .ok_or_else(|| {
                environment_error(
                    EnvironmentErrorCode::CapabilityUnavailable,
                    false,
                    "local managed preparation is absent; Brain must prepare again",
                )
            })
    }

    async fn live_workspace(
        &self,
        target: &SandboxTarget,
        expected_generation: &str,
    ) -> EnvironmentResult<Arc<LocalWorkspace>> {
        let _guard = self.target_gate.lock().await;
        let Some(current) = self.read_physical_target(target)? else {
            return Err(environment_error(
                EnvironmentErrorCode::SandboxNotMaterialized,
                false,
                "the local default target was never materialized",
            ));
        };
        if matches!(current.state.as_str(), "gone" | "terminated")
            || current.expires_at_ms <= brain::wall_ms()
        {
            return Err(environment_error(
                EnvironmentErrorCode::SandboxGone,
                false,
                "the local default target is gone",
            ));
        }
        if current.generation != expected_generation {
            return Err(environment_error(
                EnvironmentErrorCode::GenerationConflict,
                false,
                "the local file request targets a stale generation",
            ));
        }
        LocalWorkspace::open(&self.workspace_root, &Self::target_workspace_id(target))
            .map_err(brain_error_to_environment)
    }

    fn file_entry(
        workspace: &LocalWorkspace,
        requested: &str,
    ) -> EnvironmentResult<brain_protocol::environment::FileEntry> {
        let path = workspace.resolve(requested).map_err(brain_error_to_environment)?;
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                environment_error(
                    EnvironmentErrorCode::FileNotFound,
                    false,
                    "sandbox file does not exist",
                )
            } else {
                local_io_error("inspect sandbox file", error)
            }
        })?;
        let kind = if metadata.file_type().is_symlink() {
            "symlink"
        } else if metadata.is_dir() {
            "directory"
        } else if metadata.is_file() {
            "file"
        } else {
            return Err(environment_error(
                EnvironmentErrorCode::InvalidRequest,
                false,
                "sandbox path is not a regular file or directory",
            ));
        };
        let modified_at_ms = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |value| value.as_millis().min(u64::MAX as u128) as u64);
        typed(json!({
            "path": requested,
            "kind": kind,
            "bytes": if metadata.is_file() { metadata.len() } else { 0 },
            "sha256": null,
            "modified_at_ms": modified_at_ms,
        }))
    }

    fn transfer_file_path(
        authority: &brain_protocol::environment::ObjectTransferAuthority,
        expected_method: &str,
    ) -> EnvironmentResult<PathBuf> {
        if authority.method.to_string() != expected_method
            || authority.expires_at_ms.get() <= brain::wall_ms()
            || !authority.headers.is_empty()
        {
            return Err(environment_error(
                EnvironmentErrorCode::InvalidRequest,
                false,
                "local object authority has the wrong method, headers, or expiry",
            ));
        }
        let url = url::Url::parse(authority.url.as_str()).map_err(|_| {
            environment_error(
                EnvironmentErrorCode::InvalidRequest,
                false,
                "local object authority is not a valid URL",
            )
        })?;
        if url.scheme() != "file" {
            return Err(environment_error(
                EnvironmentErrorCode::CapabilityUnavailable,
                false,
                "the local Environment accepts only file object authorities",
            ));
        }
        url.to_file_path().map_err(|_| {
            environment_error(
                EnvironmentErrorCode::InvalidRequest,
                false,
                "local object authority is not a filesystem path",
            )
        })
    }

    fn operation_observation(
        operation: OperationRef,
        target: TargetReceipt,
        terminal: Option<TerminalResult>,
    ) -> EnvironmentResult<OperationObservation> {
        typed(json!({
            "operation": operation,
            "state": if terminal.is_some() { "terminal" } else { "running" },
            "output": [],
            "next_cursor": if terminal.is_some() { "terminal" } else { "running" },
            "target": target,
            "terminal": terminal,
        }))
    }

    fn validate_operation_ref(
        stored: &DurableOperation,
        supplied: &OperationRef,
    ) -> EnvironmentResult<()> {
        if serde_jcs::to_vec(&stored.operation).ok() != serde_jcs::to_vec(supplied).ok() {
            return Err(environment_error(
                EnvironmentErrorCode::OperationConflict,
                false,
                "operation receipt does not match its durable local identity",
            ));
        }
        Ok(())
    }

    fn terminal_from_outcome(outcome: CallOutcome) -> EnvironmentResult<TerminalResult> {
        let completed = outcome.outcome == EnvironmentTerminalOutcome::Completed && !outcome.is_error;
        let terminal_outcome = if completed {
            EnvironmentTerminalOutcome::Completed
        } else {
            match outcome.outcome {
                EnvironmentTerminalOutcome::Cancelled => EnvironmentTerminalOutcome::Cancelled,
                EnvironmentTerminalOutcome::DeadlineExceeded => EnvironmentTerminalOutcome::DeadlineExceeded,
                EnvironmentTerminalOutcome::Interrupted => EnvironmentTerminalOutcome::Interrupted,
                EnvironmentTerminalOutcome::Completed | EnvironmentTerminalOutcome::Failed => {
                    EnvironmentTerminalOutcome::Failed
                }
            }
        };
        let mut inline = outcome
            .value
            .or_else(|| (!outcome.content.is_empty()).then_some(Value::String(outcome.content)));
        if inline
            .as_ref()
            .is_some_and(|value| !brain_protocol::contract::terminal_inline_fits(value))
        {
            inline = Some(Value::String(
                "local managed Tool result exceeded the canonical inline terminal bound".into(),
            ));
        }
        let mut terminal: TerminalResult = typed(json!({
            "outcome": terminal_outcome,
            "terminal_digest": "0".repeat(64),
            "is_error": !completed,
            "inline": inline,
            "exit_code": outcome.exit_code,
            "duration_ms": outcome.duration_ms,
        }))?;
        terminal.terminal_digest = brain_protocol::contract::terminal_result_digest(&terminal);
        Ok(terminal)
    }

    fn interrupted_terminal() -> EnvironmentResult<TerminalResult> {
        Self::terminal_from_outcome(CallOutcome {
            outcome: EnvironmentTerminalOutcome::Interrupted,
            value: Some(json!({
                "error": "local Brain restarted after dispatch; the ambiguous effect was not repeated"
            })),
            content: String::new(),
            is_error: true,
            exit_code: None,
            duration_ms: 0,
            truncated: false,
            terminal: None,
        })
    }

    fn sandbox_execution_dir(&self, target: &SandboxTarget, execution_id: &str) -> PathBuf {
        self.root_dir(target.root_id.as_str())
            .join("sandbox-executions")
            .join(hash_component(execution_id))
    }

    fn sandbox_execution_key(target: &SandboxTarget, execution_id: &str) -> String {
        format!(
            "{}:{}",
            Self::target_digest(target),
            hash_component(execution_id)
        )
    }

    fn live_sandbox_execution(
        &self,
        target: &SandboxTarget,
        execution_id: &str,
    ) -> Option<Arc<LocalSandboxExecution>> {
        self.sandbox_executions
            .lock()
            .expect("local sandbox executions")
            .get(&Self::sandbox_execution_key(target, execution_id))
            .cloned()
    }

    fn sandbox_receipt(
        operation: OperationRef,
        observation: OperationObservation,
        replayed: bool,
    ) -> EnvironmentResult<SubmitReceipt> {
        typed(json!({
            "operation": operation,
            "replayed": replayed,
            "observation": observation,
        }))
    }

    fn durable_sandbox_observation(
        execution: &DurableSandboxExecution,
        terminal: TerminalResult,
    ) -> EnvironmentResult<OperationObservation> {
        sandbox_observation(
            execution.operation.clone(),
            execution.target.clone(),
            "",
            "",
            Some(terminal),
        )
    }

    async fn await_sandbox_terminal(
        execution: &Arc<LocalSandboxExecution>,
        timeout_ms: u64,
    ) -> EnvironmentResult<OperationObservation> {
        let deadline = tokio::time::Instant::now()
            + std::time::Duration::from_millis(timeout_ms.saturating_add(2_000));
        loop {
            let completed = execution.completed.notified();
            if execution
                .terminal
                .read()
                .expect("sandbox terminal")
                .is_some()
            {
                return execution.observation();
            }
            tokio::time::timeout_at(deadline, completed)
                .await
                .map_err(|_| {
                    environment_error(
                        EnvironmentErrorCode::TemporarilyUnavailable,
                        true,
                        "local sandbox execution terminal was not observed before its deadline",
                    )
                })?;
        }
    }

    fn insert_sandbox_execution(
        &self,
        key: String,
        execution: Arc<LocalSandboxExecution>,
    ) -> EnvironmentResult<()> {
        let mut executions = self
            .sandbox_executions
            .lock()
            .expect("local sandbox executions");
        executions.retain(|_, execution| {
            execution
                .terminal
                .read()
                .expect("sandbox terminal")
                .is_none()
        });
        if executions.len() >= MAX_LIVE_SANDBOX_EXECUTIONS {
            return Err(environment_error(
                EnvironmentErrorCode::ResourceExhausted,
                true,
                "local sandbox execution capacity is exhausted",
            ));
        }
        executions.insert(key, execution);
        Ok(())
    }

    fn cancel_sandbox_target(&self, target: &SandboxTarget) {
        let target_digest = Self::target_digest(target);
        for execution in self
            .sandbox_executions
            .lock()
            .expect("local sandbox executions")
            .values()
        {
            if Self::target_digest(&execution.operation.target) == target_digest {
                execution.cancel.cancel();
            }
        }
    }
}

fn sandbox_observation(
    operation: OperationRef,
    target: TargetReceipt,
    stdout: &str,
    stderr: &str,
    terminal: Option<TerminalResult>,
) -> EnvironmentResult<OperationObservation> {
    let mut output = Vec::new();
    append_output_chunks(&mut output, "stdout", stdout);
    append_output_chunks(&mut output, "stderr", stderr);
    let cursor = format!("output_{}", stdout.len().saturating_add(stderr.len()));
    typed(json!({
        "operation": operation,
        "state": if terminal.is_some() { "terminal" } else { "running" },
        "output": output,
        "next_cursor": cursor,
        "target": target,
        "terminal": terminal,
    }))
}

fn append_output_chunks(output: &mut Vec<Value>, stream: &str, text: &str) {
    let mut start = 0;
    let mut chars = 0usize;
    for (index, _) in text.char_indices() {
        if chars == 4_096 {
            output.push(json!({
                "stream": stream,
                "offset": start,
                "text": &text[start..index],
            }));
            start = index;
            chars = 0;
        }
        chars += 1;
    }
    if start < text.len() {
        output.push(json!({
            "stream": stream,
            "offset": start,
            "text": &text[start..],
        }));
    }
}

fn sandbox_terminal_result(
    status: Option<std::process::ExitStatus>,
    cancelled: bool,
    timed_out: bool,
    stdout: String,
    stderr: String,
    duration_ms: u64,
) -> EnvironmentResult<TerminalResult> {
    let exit_code = status.and_then(|status| status.code()).map(i64::from);
    let (outcome, is_error) = if cancelled {
        ("cancelled", true)
    } else if timed_out {
        ("deadline_exceeded", true)
    } else if status.is_some_and(|status| status.success()) {
        ("completed", false)
    } else {
        ("failed", true)
    };
    let mut inline = json!({
        "stdout": stdout,
        "stderr": stderr,
    });
    if !brain_protocol::contract::terminal_inline_fits(&inline) {
        inline = json!({"output_truncated": true});
    }
    let mut terminal: TerminalResult = typed(json!({
        "outcome": outcome,
        "terminal_digest": "0".repeat(64),
        "is_error": is_error,
        "inline": inline,
        "exit_code": exit_code,
        "duration_ms": duration_ms,
    }))?;
    terminal.terminal_digest = brain_protocol::contract::terminal_result_digest(&terminal);
    Ok(terminal)
}

#[async_trait]
impl EnvironmentPort for LocalEnvironment {
    async fn resolve_binding(&self, binding: SealedBinding) -> EnvironmentResult<ResolvedBinding> {
        if binding.bundle.is_none()
            || !binding.required_capabilities.iter().all(|capability| {
                matches!(
                    capability,
                    brain_protocol::environment::EnvironmentCapability::Execution
                        | brain_protocol::environment::EnvironmentCapability::SessionPreparation
                )
            })
        {
            return Err(environment_error(
                EnvironmentErrorCode::CapabilityUnavailable,
                false,
                "the local Environment accepts only sealed computer-artifact bindings",
            ));
        }
        let descriptor = binding.bundle.as_ref().expect("checked");
        if descriptor.target != brain_protocol::environment::ArtifactTarget::LinuxAmd64
            || descriptor.bundle_digest != binding.implementation_identity
            || descriptor.contract_digest != binding.contract_digest
            || descriptor.tool_name != binding.capability
            || descriptor.bytes.get() as usize > brain_protocol::MAX_TOOL_BUNDLE_BYTES
            || descriptor.object.bytes != descriptor.bytes.get()
            || descriptor.object.sha256 != descriptor.bundle_digest
        {
            return Err(environment_error(
                EnvironmentErrorCode::BindingConflict,
                false,
                "managed binding and bundle descriptor disagree",
            ));
        }
        let path = self.binding_path(&binding);
        if let Some(existing) = read_json_if_exists::<SealedBinding>(&path, "managed binding")? {
            if serde_jcs::to_vec(&existing).ok() != serde_jcs::to_vec(&binding).ok() {
                return Err(environment_error(
                    EnvironmentErrorCode::BindingConflict,
                    false,
                    "binding_ref is already sealed to different bytes",
                ));
            }
        } else {
            write_new_json(&path, &binding, "persist managed binding")?;
        }
        self.bindings
            .write()
            .expect("local bindings")
            .insert(binding.binding_id.to_string(), binding.clone());
        typed(json!({
            "binding_ref": binding.binding_id,
            "environment_id": "environment_local",
            "recovery": "retained",
            "capabilities": ["execution", "session_preparation"],
            "limits": {
                "max_inline_input_bytes": brain_protocol::MAX_MESSAGE_REQUEST_BYTES,
                "max_inline_result_bytes": brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
                "max_wait_ms": 30_000,
            }
        }))
    }

    async fn submit(&self, request: SubmitRequest) -> EnvironmentResult<SubmitReceipt> {
        if brain_protocol::contract::operation_request_digest(&request.envelope)
            != request.envelope.request_digest
        {
            return Err(environment_error(
                EnvironmentErrorCode::OperationConflict,
                false,
                "managed request digest does not match its canonical envelope",
            ));
        }
        let session_id = request.envelope.session_id.to_string();
        let root_id = request.envelope.root_id.to_string();
        let operation_id = request.envelope.operation_id.to_string();
        let runtime = self.prepared_runtime(&session_id)?;
        if runtime.root_id != root_id
            || !runtime
                .binding_refs
                .contains(request.envelope.binding_ref.as_str())
        {
            return Err(environment_error(
                EnvironmentErrorCode::BindingConflict,
                false,
                "managed operation is outside its prepared root or binding",
            ));
        }
        let gate = self.operation_gate(&root_id, &operation_id);
        let _guard = gate.lock().await;
        let dir = self.operation_dir(&root_id, &operation_id);
        let request_path = dir.join("request.json");
        let terminal_path = dir.join("terminal.json");
        let ack_path = dir.join("ack.json");
        if ack_path.exists() {
            return Err(environment_error(
                EnvironmentErrorCode::OperationUnknown,
                false,
                "the local operation terminal was already acknowledged",
            ));
        }
        if let Some(stored) =
            read_json_if_exists::<DurableOperation>(&request_path, "managed operation request")?
        {
            if serde_jcs::to_vec(&stored.envelope).ok() != serde_jcs::to_vec(&request.envelope).ok()
            {
                return Err(environment_error(
                    EnvironmentErrorCode::OperationConflict,
                    false,
                    "operation_id is already sealed to a different request",
                ));
            }
            let terminal = match read_json_if_exists::<TerminalResult>(
                &terminal_path,
                "managed operation terminal",
            )? {
                Some(terminal) => terminal,
                None => {
                    let terminal = Self::interrupted_terminal()?;
                    write_new_json(
                        &terminal_path,
                        &terminal,
                        "persist interrupted managed terminal",
                    )?;
                    terminal
                }
            };
            let observation = Self::operation_observation(
                stored.operation.clone(),
                stored.target,
                Some(terminal),
            )?;
            return typed(json!({
                "operation": stored.operation,
                "replayed": true,
                "observation": observation,
            }));
        }

        let target: SandboxTarget = typed(json!({
            "kind": "default",
            "session_id": request.envelope.session_id,
            "root_id": request.envelope.root_id,
            "binding_ref": request.envelope.binding_ref,
        }))?;
        let (status, _) = self.ensure_target(target.clone(), None).await?;
        let generation = status.generation.as_ref().ok_or_else(|| {
            environment_error(
                EnvironmentErrorCode::SandboxGone,
                false,
                "local target lacks a generation",
            )
        })?;
        let target_ref = status.target_ref.as_ref().ok_or_else(|| {
            environment_error(
                EnvironmentErrorCode::SandboxGone,
                false,
                "local target lacks a physical reference",
            )
        })?;
        if request
            .envelope
            .generation
            .as_ref()
            .is_some_and(|expected| expected.as_str() != generation.as_str())
            || request
                .envelope
                .target_ref
                .as_ref()
                .is_some_and(|expected| expected.as_str() != target_ref.as_str())
        {
            return Err(environment_error(
                EnvironmentErrorCode::GenerationConflict,
                false,
                "managed request targets a stale local generation",
            ));
        }
        let receipt_digest = hash_component(&format!(
            "{}\0{}\0{}\0{}",
            operation_id,
            request.envelope.request_digest.as_str(),
            target_ref.as_str(),
            generation.as_str()
        ));
        let operation: OperationRef = typed(json!({
            "operation_id": request.envelope.operation_id,
            "request_digest": request.envelope.request_digest,
            "target": target,
            "generation": generation,
            "target_ref": target_ref,
            "receipt_ref": format!("receipt_{}", &receipt_digest[..32]),
        }))?;
        let target_receipt: TargetReceipt = typed(json!({
            "target_ref": target_ref,
            "generation": generation,
            "expires_at_ms": status.expires_at_ms,
        }))?;
        let durable = DurableOperation {
            envelope: request.envelope.clone(),
            operation: operation.clone(),
            target: target_receipt.clone(),
        };
        write_new_json(&request_path, &durable, "reserve managed operation")?;

        if brain::wall_ms() >= request.envelope.deadline_at_ms.get() {
            let terminal = Self::terminal_from_outcome(CallOutcome {
                outcome: EnvironmentTerminalOutcome::DeadlineExceeded,
                value: Some(json!({"error":"managed Tool deadline elapsed before execution"})),
                content: String::new(),
                is_error: true,
                exit_code: None,
                duration_ms: 0,
                truncated: false,
                terminal: None,
            })?;
            write_new_json(&terminal_path, &terminal, "persist managed terminal")?;
            let observation =
                Self::operation_observation(operation.clone(), target_receipt, Some(terminal))?;
            return typed(json!({
                "operation": operation,
                "replayed": false,
                "observation": observation,
            }));
        }

        let cancel = CancellationToken::new();
        self.running
            .lock()
            .expect("local running operations")
            .insert(operation_id.clone(), cancel.clone());
        let outcome = runtime
            .workspace
            .execute_operation(
                request.envelope.capability.as_str(),
                request.envelope.input.value.clone(),
                &operation_id,
                request.envelope.deadline_at_ms.get(),
                &cancel,
                |_, _, _| {},
            )
            .await;
        self.running
            .lock()
            .expect("local running operations")
            .remove(&operation_id);
        let terminal = Self::terminal_from_outcome(outcome)?;
        write_new_json(&terminal_path, &terminal, "persist managed terminal")?;
        let observation =
            Self::operation_observation(operation.clone(), target_receipt, Some(terminal))?;
        typed(json!({
            "operation": operation,
            "replayed": false,
            "observation": observation,
        }))
    }

    async fn observe(&self, request: ObserveRequest) -> EnvironmentResult<OperationObservation> {
        let dir = self.operation_dir(
            request.operation.target.root_id.as_str(),
            request.operation.operation_id.as_str(),
        );
        if dir.join("ack.json").exists() {
            return Err(environment_error(
                EnvironmentErrorCode::OperationUnknown,
                false,
                "the local operation terminal was already acknowledged",
            ));
        }
        let stored: DurableOperation = read_json(&dir.join("request.json"), "managed operation")?;
        Self::validate_operation_ref(&stored, &request.operation)?;
        let terminal = read_json_if_exists(&dir.join("terminal.json"), "managed terminal")?;
        Self::operation_observation(stored.operation, stored.target, terminal)
    }

    async fn cancel(&self, request: CancelRequest) -> EnvironmentResult<CancellationReceipt> {
        let dir = self.operation_dir(
            request.operation.target.root_id.as_str(),
            request.operation.operation_id.as_str(),
        );
        let stored: DurableOperation = read_json(&dir.join("request.json"), "managed operation")?;
        Self::validate_operation_ref(&stored, &request.operation)?;
        let accepted = if let Some(cancel) = self
            .running
            .lock()
            .expect("local running operations")
            .get(request.operation.operation_id.as_str())
            .cloned()
        {
            cancel.cancel();
            true
        } else {
            false
        };
        let terminal = read_json_if_exists(&dir.join("terminal.json"), "managed terminal")?;
        let observation =
            Self::operation_observation(stored.operation.clone(), stored.target, terminal)?;
        typed(json!({
            "operation": stored.operation,
            "accepted": accepted,
            "observation": observation,
        }))
    }

    async fn acknowledge_terminal(
        &self,
        request: AcknowledgeTerminalRequest,
    ) -> EnvironmentResult<Acknowledgement> {
        let root_id = request.operation.target.root_id.to_string();
        let operation_id = request.operation.operation_id.to_string();
        let gate = self.operation_gate(&root_id, &operation_id);
        let _guard = gate.lock().await;
        let dir = self.operation_dir(&root_id, &operation_id);
        let ack_path = dir.join("ack.json");
        if let Some(existing) = read_json_if_exists::<AcknowledgeTerminalRequest>(
            &ack_path,
            "managed terminal acknowledgement",
        )? {
            if serde_jcs::to_vec(&existing).ok() != serde_jcs::to_vec(&request).ok() {
                return Err(environment_error(
                    EnvironmentErrorCode::OperationConflict,
                    false,
                    "terminal acknowledgement conflicts with its durable tombstone",
                ));
            }
            return typed(json!({"acknowledged": true}));
        }
        let stored: DurableOperation = read_json(&dir.join("request.json"), "managed operation")?;
        Self::validate_operation_ref(&stored, &request.operation)?;
        let terminal: TerminalResult = read_json(&dir.join("terminal.json"), "managed terminal")?;
        if terminal.terminal_digest != request.terminal_digest {
            return Err(environment_error(
                EnvironmentErrorCode::OperationConflict,
                false,
                "terminal acknowledgement digest does not match the retained terminal",
            ));
        }
        write_new_json(&ack_path, &request, "persist terminal acknowledgement")?;
        match std::fs::remove_file(dir.join("terminal.json")) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(local_io_error(
                    "discard acknowledged terminal payload",
                    error,
                ));
            }
        }
        typed(json!({"acknowledged": true}))
    }
}

#[async_trait]
impl SessionPreparationPort for LocalEnvironment {
    async fn prepare(&self, request: PrepareSessionRequest) -> EnvironmentResult<PreparedSession> {
        let session_id = request.session_id.to_string();
        let root_id = request.root_id.to_string();
        let mut specs = Vec::with_capacity(request.bindings.len());
        let mut binding_refs = std::collections::HashSet::new();
        let mut expected_env = std::collections::BTreeSet::new();
        let mut fetches = HashMap::new();
        for fetch in &request.bundles {
            if fetch.expires_at_ms.get() <= brain::wall_ms()
                || fetch.max_bytes.get() as usize > brain_protocol::MAX_TOOL_BUNDLE_BYTES
                || fetches
                    .insert(fetch.bundle_digest.to_string(), fetch)
                    .is_some()
            {
                return Err(environment_error(
                    EnvironmentErrorCode::BindingConflict,
                    false,
                    "preparation bundle fetch is expired, duplicated, or oversized",
                ));
            }
        }
        let physical_id = hash_component(&root_id);
        let workspace_base = self.workspace_root.join(&physical_id);
        let bundle_dir = workspace_base.join("bundles");
        std::fs::create_dir_all(&bundle_dir)
            .map_err(|error| local_io_error("create local bundle cache", error))?;
        for prepared in &request.bindings {
            if !binding_refs.insert(prepared.binding_ref.to_string()) {
                return Err(environment_error(
                    EnvironmentErrorCode::BindingConflict,
                    false,
                    "preparation contains a duplicate binding_ref",
                ));
            }
            let binding = self.binding(prepared.binding_ref.as_str())?;
            if binding.root_id.as_str() != root_id || binding.session_id.as_str() != session_id {
                return Err(environment_error(
                    EnvironmentErrorCode::BindingConflict,
                    false,
                    "preparation binding belongs to another root or session",
                ));
            }
            let descriptor = binding.bundle.as_ref().ok_or_else(|| {
                environment_error(
                    EnvironmentErrorCode::BindingConflict,
                    false,
                    "prepared managed binding has no bundle",
                )
            })?;
            if prepared.bundle_digests.len() != 1
                || prepared.bundle_digests[0] != descriptor.bundle_digest
            {
                return Err(environment_error(
                    EnvironmentErrorCode::BindingConflict,
                    false,
                    "prepared binding does not carry its exact sealed bundle digest",
                ));
            }
            let fetch = fetches
                .get(descriptor.bundle_digest.as_str())
                .ok_or_else(|| {
                    environment_error(
                        EnvironmentErrorCode::CapabilityUnavailable,
                        false,
                        "preparation omitted a required bundle fetch",
                    )
                })?;
            let source = url::Url::parse(fetch.url.as_str()).map_err(|_| {
                environment_error(
                    EnvironmentErrorCode::InvalidRequest,
                    false,
                    "local bundle fetch is not a valid URL",
                )
            })?;
            if source.scheme() != "file" || !fetch.headers.is_empty() {
                return Err(environment_error(
                    EnvironmentErrorCode::CapabilityUnavailable,
                    false,
                    "the local Environment accepts only header-free file bundle authorities",
                ));
            }
            let source = source.to_file_path().map_err(|_| {
                environment_error(
                    EnvironmentErrorCode::InvalidRequest,
                    false,
                    "local bundle authority is not a filesystem path",
                )
            })?;
            let metadata = std::fs::metadata(&source)
                .map_err(|error| local_io_error("inspect local bundle", error))?;
            if metadata.len() != descriptor.bytes.get()
                || metadata.len() > fetch.max_bytes.get()
                || metadata.len() as usize > brain_protocol::MAX_TOOL_BUNDLE_BYTES
            {
                return Err(environment_error(
                    EnvironmentErrorCode::BindingConflict,
                    false,
                    "local bundle bytes disagree with their sealed descriptor",
                ));
            }
            let bytes = std::fs::read(&source)
                .map_err(|error| local_io_error("read local bundle", error))?;
            if hex::encode(sha2::Sha256::digest(&bytes)) != descriptor.bundle_digest.as_str() {
                return Err(environment_error(
                    EnvironmentErrorCode::BindingConflict,
                    false,
                    "local bundle checksum disagrees with its sealed descriptor",
                ));
            }
            let destination = bundle_dir.join(format!("{}.mjs", descriptor.bundle_digest.as_str()));
            if let Some(existing) = read_bytes_if_exists(&destination, "local bundle cache")? {
                if existing != bytes {
                    return Err(environment_error(
                        EnvironmentErrorCode::BindingConflict,
                        false,
                        "immutable local bundle cache contains different bytes",
                    ));
                }
            } else {
                write_new_bytes(&destination, &bytes, "cache local bundle")?;
            }
            for name in &descriptor.required_env {
                expected_env.insert(name.to_string());
            }
            specs.push(LocalManagedSpec {
                name: descriptor.tool_name.to_string(),
                description: descriptor
                    .description
                    .as_ref()
                    .map(|value| value.as_str().to_owned()),
                contract_digest: descriptor.contract_digest.to_string(),
                bundle_digest: descriptor.bundle_digest.to_string(),
                required_env: descriptor
                    .required_env
                    .iter()
                    .map(|value| value.as_str().to_owned())
                    .collect(),
            });
        }
        if fetches.len() != specs.len() {
            let used: std::collections::HashSet<_> = specs
                .iter()
                .map(|spec| spec.bundle_digest.as_str())
                .collect();
            if fetches.keys().any(|digest| !used.contains(digest.as_str())) {
                return Err(environment_error(
                    EnvironmentErrorCode::BindingConflict,
                    false,
                    "preparation contains an unreferenced bundle fetch",
                ));
            }
        }
        let env = match (&request.secret_capability, expected_env.is_empty()) {
            (None, true) => HashMap::new(),
            (None, false) => {
                return Err(environment_error(
                    EnvironmentErrorCode::CapabilityUnavailable,
                    false,
                    "preparation omitted required managed secret material",
                ));
            }
            (Some(capability), _) => {
                let named: std::collections::BTreeSet<_> = capability
                    .env_names
                    .iter()
                    .map(|value| value.as_str().to_owned())
                    .collect();
                if named != expected_env || capability.expires_at_ms.get() <= brain::wall_ms() {
                    return Err(environment_error(
                        EnvironmentErrorCode::BindingConflict,
                        false,
                        "secret capability does not match the exact prepared environment",
                    ));
                }
                let delivery = self
                    .secret_delivery
                    .read()
                    .expect("local secret delivery")
                    .clone()
                    .ok_or_else(|| {
                        environment_error(
                            EnvironmentErrorCode::TemporarilyUnavailable,
                            true,
                            "local secret delivery is not attached",
                        )
                    })?;
                let first_binding = request.bindings.first().ok_or_else(|| {
                    environment_error(
                        EnvironmentErrorCode::BindingConflict,
                        false,
                        "secret capability requires a prepared binding",
                    )
                })?;
                let generation = format!("prep_{}", &hash_component(&session_id)[..24]);
                let secret_request = typed(json!({
                    "capability_ref": capability.capability_ref,
                    "generation_intent": generation,
                    "environment_id": "environment_local",
                    "root_id": request.root_id,
                    "session_id": request.session_id,
                    "target": {
                        "kind": "default",
                        "session_id": request.session_id,
                        "root_id": request.root_id,
                        "binding_ref": first_binding.binding_ref,
                    }
                }))?;
                let values = delivery.redeem(secret_request).await?.into_env();
                if values
                    .keys()
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>()
                    != expected_env
                    || values.len() > brain_protocol::MAX_SESSION_SECRET_NAMES
                    || values.values().any(|value| {
                        value.len() > brain_protocol::MAX_SESSION_SECRET_VALUE_UTF8_BYTES
                    })
                {
                    return Err(environment_error(
                        EnvironmentErrorCode::BindingConflict,
                        false,
                        "redeemed secret material does not match its bounded capability",
                    ));
                }
                values
            }
        };
        let workspace = LocalWorkspace::open_with_runtime(
            &self.workspace_root,
            &physical_id,
            &session_id,
            specs,
            env,
        )
        .map_err(brain_error_to_environment)?;
        self.prepared
            .write()
            .expect("local prepared sessions")
            .insert(
                session_id.clone(),
                PreparedRuntime {
                    root_id: root_id.clone(),
                    binding_refs,
                    workspace,
                },
            );
        let preparation_digest = hash_component(&format!("{root_id}\0{session_id}"));
        typed(json!({
            "preparation_ref": format!("prep_{}", &preparation_digest[..24]),
        }))
    }

    async fn materialize_default(
        &self,
        request: CreateSandboxRequest,
    ) -> EnvironmentResult<SandboxStatus> {
        self.ensure_target(request.target, Some(request.generation_intent.as_str()))
            .await
            .map(|(status, _)| status)
    }

    async fn dematerialize_default(&self, target: SandboxTarget) -> EnvironmentResult<SandboxStatus> {
        if target.kind != brain_protocol::environment::TargetKind::Default || target.sandbox_id.is_some() {
            return Err(environment_error(
                EnvironmentErrorCode::InvalidRequest,
                false,
                "default dematerialization requires the shared default target",
            ));
        }
        let _guard = self.target_gate.lock().await;
        let Some(current) = self.read_physical_target(&target)? else {
            return Err(environment_error(
                EnvironmentErrorCode::SandboxNotMaterialized,
                false,
                "the local default target was never materialized",
            ));
        };
        if matches!(current.state.as_str(), "gone" | "terminated") {
            return self.status_for(target, &current);
        }
        let terminal = PhysicalTargetState {
            state: "terminated".into(),
            changed_at_ms: brain::wall_ms(),
            ..current
        };
        self.append_physical_target(&target, &terminal)?;
        self.status_for(target, &terminal)
    }

    async fn purge_tree(&self, root_id: &str) -> EnvironmentResult<()> {
        let mut executions = self
            .sandbox_executions
            .lock()
            .expect("local sandbox executions");
        executions.retain(|_, execution| {
            if execution.operation.target.root_id.as_str() == root_id {
                execution.cancel.cancel();
                false
            } else {
                true
            }
        });
        drop(executions);
        self.prepared
            .write()
            .expect("local prepared sessions")
            .retain(|_, runtime| runtime.root_id != root_id);
        for path in [
            self.root_dir(root_id),
            self.workspace_root.join(hash_component(root_id)),
        ] {
            match std::fs::remove_dir_all(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(local_io_error("purge local Environment tree", error)),
            }
        }
        Ok(())
    }
}

#[async_trait]
impl SandboxControlPort for LocalEnvironment {
    async fn create(&self, request: CreateSandboxRequest) -> EnvironmentResult<SandboxStatus> {
        if request.target.kind != brain_protocol::environment::TargetKind::Additional
            || request.target.sandbox_id.is_none()
        {
            return Err(environment_error(
                EnvironmentErrorCode::InvalidRequest,
                false,
                "additional sandbox creation requires an additional target and sandbox_id",
            ));
        }
        self.ensure_target(request.target, Some(request.generation_intent.as_str()))
            .await
            .map(|(status, _)| status)
    }

    async fn inspect(&self, target: SandboxTarget) -> EnvironmentResult<SandboxStatus> {
        if target.kind != brain_protocol::environment::TargetKind::Additional
            || target.sandbox_id.is_none()
        {
            return Err(environment_error(
                EnvironmentErrorCode::InvalidRequest,
                false,
                "additional sandbox inspection requires an additional target and sandbox_id",
            ));
        }
        <Self as SandboxFilesPort>::status(self, target).await
    }

    async fn execute(&self, request: SandboxExecutionRequest) -> EnvironmentResult<SubmitReceipt> {
        if brain_protocol::contract::sandbox_execution_request_digest(&request)
            != request.request_digest
        {
            return Err(environment_error(
                EnvironmentErrorCode::InvalidRequest,
                false,
                "sandbox execution request digest is invalid",
            ));
        }
        if request.target.kind != brain_protocol::environment::TargetKind::Additional
            || request.target.sandbox_id.is_none()
        {
            return Err(environment_error(
                EnvironmentErrorCode::InvalidRequest,
                false,
                "sandbox execution requires an additional target and sandbox_id",
            ));
        }
        if request.resources.max_output_bytes.get() as usize
            > brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES
        {
            return Err(environment_error(
                EnvironmentErrorCode::ResourceExhausted,
                false,
                "sandbox execution output ceiling exceeds the Brain terminal projection",
            ));
        }

        let target = request.target.clone();
        let execution_id = request.execution_id.to_string();
        let gate = self.operation_gate(target.root_id.as_str(), &execution_id);
        let _guard = gate.lock().await;
        let execution_dir = self.sandbox_execution_dir(&target, &execution_id);
        let request_path = execution_dir.join("request.json");
        let terminal_path = execution_dir.join("terminal.json");
        if let Some(stored) = read_json_if_exists::<DurableSandboxExecution>(
            &request_path,
            "sandbox execution request",
        )? {
            if serde_jcs::to_vec(&stored.request).ok() != serde_jcs::to_vec(&request).ok() {
                return Err(environment_error(
                    EnvironmentErrorCode::OperationConflict,
                    false,
                    "sandbox execution_id is already sealed to a different request",
                ));
            }
            if let Some(live) = self.live_sandbox_execution(&target, &execution_id) {
                return Self::sandbox_receipt(stored.operation, live.observation()?, true);
            }
            if let Some(terminal) =
                read_json_if_exists::<TerminalResult>(&terminal_path, "sandbox terminal")?
            {
                let observation = Self::durable_sandbox_observation(&stored, terminal)?;
                return Self::sandbox_receipt(stored.operation, observation, true);
            }
            return Err(environment_error(
                EnvironmentErrorCode::OperationUnknown,
                false,
                "local Brain restarted after sandbox dispatch; the effect will not be repeated",
            ));
        }

        let status = self.inspect(target.clone()).await?;
        if !matches!(
            status.state,
            brain_protocol::environment::SandboxState::Running
                | brain_protocol::environment::SandboxState::Suspended
        ) || status.generation.as_ref().map(|value| value.as_str())
            != Some(request.expected_generation.as_str())
        {
            return Err(environment_error(
                EnvironmentErrorCode::GenerationConflict,
                false,
                "sandbox execution targets a stale or non-running generation",
            ));
        }
        let target_ref = status.target_ref.clone().ok_or_else(|| {
            environment_error(
                EnvironmentErrorCode::SandboxGone,
                false,
                "live local sandbox has no target reference",
            )
        })?;
        let expires_at_ms = status.expires_at_ms.ok_or_else(|| {
            environment_error(
                EnvironmentErrorCode::SandboxGone,
                false,
                "live local sandbox has no hard expiry",
            )
        })?;
        let workspace = self
            .live_workspace(&target, request.expected_generation.as_str())
            .await?;
        let cwd = match request.input.cwd.as_ref() {
            Some(cwd) if !cwd.is_empty() => workspace
                .resolve(cwd.as_str())
                .map_err(brain_error_to_environment)?,
            _ => workspace.root.clone(),
        };
        if !cwd.is_dir() {
            return Err(environment_error(
                EnvironmentErrorCode::InvalidRequest,
                false,
                "sandbox execution cwd is not a directory",
            ));
        }
        let slot = self
            .sandbox_execution_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                environment_error(
                    EnvironmentErrorCode::ResourceExhausted,
                    true,
                    "local sandbox execution capacity is exhausted",
                )
            })?;

        let receipt_digest = hash_component(&format!(
            "local-sandbox-receipt\0{}\0{}\0{}",
            execution_id,
            request.request_digest.as_str(),
            target_ref.as_str()
        ));
        let operation: OperationRef = typed(json!({
            "operation_id": request.execution_id,
            "request_digest": request.request_digest,
            "target": target,
            "generation": request.expected_generation,
            "target_ref": target_ref,
            "receipt_ref": format!("receipt_{}", &receipt_digest[..24]),
        }))?;
        let target_receipt: TargetReceipt = typed(json!({
            "generation": operation.generation,
            "target_ref": operation.target_ref,
            "expires_at_ms": expires_at_ms,
        }))?;
        let durable = DurableSandboxExecution {
            request: request.clone(),
            operation: operation.clone(),
            target: target_receipt.clone(),
        };
        write_new_json(&request_path, &durable, "reserve local sandbox execution")?;

        let mut command = tokio::process::Command::new("bash");
        configure_minimal_process_env(&mut command);
        command
            .args(["-lc", request.input.command.as_str()])
            .current_dir(cwd)
            .stdin(if request.input.interactive {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&execution_dir);
                return Err(environment_error(
                    EnvironmentErrorCode::TemporarilyUnavailable,
                    true,
                    &format!("could not spawn local sandbox bash: {error}"),
                ));
            }
        };
        let stdin = child.stdin.take();
        let mut stdout_pipe = child.stdout.take().expect("sandbox stdout is piped");
        let mut stderr_pipe = child.stderr.take().expect("sandbox stderr is piped");
        let output_limit = request.resources.max_output_bytes.get() as usize;
        let stdout = Arc::new(std::sync::Mutex::new(StreamBuf::bounded(
            output_limit.saturating_div(2).max(1),
        )));
        let stderr = Arc::new(std::sync::Mutex::new(StreamBuf::bounded(
            output_limit.saturating_sub(output_limit.saturating_div(2)),
        )));
        let execution = Arc::new(LocalSandboxExecution {
            operation: operation.clone(),
            target: target_receipt,
            stdout: stdout.clone(),
            stderr: stderr.clone(),
            terminal: std::sync::RwLock::new(None),
            stdin: tokio::sync::Mutex::new(stdin),
            cancel: CancellationToken::new(),
            completed: tokio::sync::Notify::new(),
            _slot: slot,
        });
        let execution_key = Self::sandbox_execution_key(&target, &execution_id);
        if let Err(error) = self.insert_sandbox_execution(execution_key, execution.clone()) {
            execution.cancel.cancel();
            kill_tree(&mut child).await;
            let _ = std::fs::remove_dir_all(&execution_dir);
            return Err(error);
        }

        let out_task = {
            let stdout = stdout.clone();
            tokio::spawn(async move {
                collect_stream(&mut stdout_pipe, "stdout", &|_, _, _| {}, &stdout).await
            })
        };
        let err_task = {
            let stderr = stderr.clone();
            tokio::spawn(async move {
                collect_stream(&mut stderr_pipe, "stderr", &|_, _, _| {}, &stderr).await
            })
        };
        let timeout_ms = request.resources.timeout_ms.get();
        let started = Instant::now();
        let background = execution.clone();
        tokio::spawn(async move {
            let mut cancelled = false;
            let mut timed_out = false;
            let status = tokio::select! {
                status = child.wait() => status.ok(),
                () = background.cancel.cancelled() => {
                    cancelled = true;
                    kill_tree(&mut child).await;
                    child.wait().await.ok()
                }
                () = tokio::time::sleep(std::time::Duration::from_millis(timeout_ms)) => {
                    timed_out = true;
                    kill_tree(&mut child).await;
                    child.wait().await.ok()
                }
            };
            let drain = std::time::Duration::from_millis(1_500);
            let _ = tokio::time::timeout(drain, out_task).await;
            let _ = tokio::time::timeout(drain, err_task).await;
            background.stdin.lock().await.take();
            let stdout = background.stdout.lock().expect("sandbox stdout").render();
            let stderr = background.stderr.lock().expect("sandbox stderr").render();
            if let Ok(terminal) = sandbox_terminal_result(
                status,
                cancelled,
                timed_out,
                stdout,
                stderr,
                started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            ) {
                let _ = write_new_json(&terminal_path, &terminal, "persist sandbox terminal");
                *background.terminal.write().expect("sandbox terminal") = Some(terminal);
            }
            background.completed.notify_waiters();
        });

        let observation = if request.input.interactive {
            execution.observation()?
        } else {
            Self::await_sandbox_terminal(&execution, timeout_ms).await?
        };
        Self::sandbox_receipt(operation, observation, false)
    }

    async fn write_stdin(&self, request: WriteStdinRequest) -> EnvironmentResult<WriteStdinReceipt> {
        if brain_protocol::contract::write_stdin_request_digest(&request) != request.request_digest
            || request.text.len() > 4_096
        {
            return Err(environment_error(
                EnvironmentErrorCode::InvalidRequest,
                false,
                "sandbox stdin request or digest is invalid",
            ));
        }
        let root_id = request.target.root_id.to_string();
        let execution_id = request.execution_id.to_string();
        let operation_id = request.operation_id.to_string();
        let gate = self.operation_gate(&root_id, &operation_id);
        let _guard = gate.lock().await;
        let execution_dir = self.sandbox_execution_dir(&request.target, &execution_id);
        let durable: DurableSandboxExecution =
            read_json(&execution_dir.join("request.json"), "sandbox execution")?;
        if durable.operation.target_ref.as_str()
            != self
                .inspect(request.target.clone())
                .await?
                .target_ref
                .as_ref()
                .map(|value| value.as_str())
                .unwrap_or_default()
            || serde_jcs::to_vec(&durable.operation.target).ok()
                != serde_jcs::to_vec(&request.target).ok()
            || durable.operation.generation.as_str() != request.expected_generation.as_str()
            || durable.operation.operation_id.as_str() != execution_id
        {
            return Err(environment_error(
                EnvironmentErrorCode::GenerationConflict,
                false,
                "sandbox stdin targets a different execution or generation",
            ));
        }
        let effect_dir = execution_dir
            .join("stdin")
            .join(hash_component(&operation_id));
        let identity_path = effect_dir.join("identity.json");
        let result_path = effect_dir.join("result.json");
        if let Some(identity) = read_json_if_exists::<DurableFileEffectIdentity>(
            &identity_path,
            "sandbox stdin identity",
        )? {
            if identity.kind != "stdin"
                || identity.request_digest != request.request_digest.as_str()
            {
                return Err(environment_error(
                    EnvironmentErrorCode::OperationConflict,
                    false,
                    "sandbox stdin operation_id is sealed to a different request",
                ));
            }
            let result =
                read_json_if_exists::<DurableStdinEffect>(&result_path, "sandbox stdin result")?
                    .ok_or_else(|| {
                        environment_error(
                            EnvironmentErrorCode::OperationUnknown,
                            false,
                            "sandbox stdin delivery is ambiguous and will not be repeated",
                        )
                    })?;
            let observation =
                if let Some(live) = self.live_sandbox_execution(&request.target, &execution_id) {
                    live.observation()?
                } else {
                    let terminal: TerminalResult =
                        read_json(&execution_dir.join("terminal.json"), "sandbox terminal")?;
                    Self::durable_sandbox_observation(&durable, terminal)?
                };
            return typed(json!({
                "operation_id": operation_id,
                "request_digest": request.request_digest,
                "accepted": result.accepted,
                "replayed": true,
                "observation": observation,
            }));
        }
        write_new_json(
            &identity_path,
            &DurableFileEffectIdentity {
                kind: "stdin".into(),
                request_digest: request.request_digest.to_string(),
            },
            "reserve sandbox stdin effect",
        )?;
        let pure_poll = request.text.is_empty() && !request.eof;
        let Some(live) = self.live_sandbox_execution(&request.target, &execution_id) else {
            if pure_poll
                && let Some(terminal) = read_json_if_exists::<TerminalResult>(
                    &execution_dir.join("terminal.json"),
                    "sandbox terminal",
                )?
            {
                let result = DurableStdinEffect {
                    request_digest: request.request_digest.to_string(),
                    accepted: false,
                };
                write_new_json(&result_path, &result, "persist sandbox stdin poll")?;
                let observation = Self::durable_sandbox_observation(&durable, terminal)?;
                return typed(json!({
                    "operation_id": operation_id,
                    "request_digest": request.request_digest,
                    "accepted": false,
                    "replayed": false,
                    "observation": observation,
                }));
            }
            return Err(environment_error(
                EnvironmentErrorCode::OperationUnknown,
                false,
                "interactive sandbox execution is no longer live",
            ));
        };
        let accepted = !pure_poll;
        if accepted {
            let mut stdin = live.stdin.lock().await;
            let pipe = stdin.as_mut().ok_or_else(|| {
                environment_error(
                    EnvironmentErrorCode::OperationUnknown,
                    false,
                    "interactive sandbox stdin is already closed",
                )
            })?;
            if !request.text.is_empty() {
                pipe.write_all(request.text.as_bytes()).await.map_err(|_| {
                    environment_error(
                        EnvironmentErrorCode::OperationUnknown,
                        false,
                        "sandbox stdin delivery became ambiguous",
                    )
                })?;
                pipe.flush().await.map_err(|_| {
                    environment_error(
                        EnvironmentErrorCode::OperationUnknown,
                        false,
                        "sandbox stdin delivery became ambiguous",
                    )
                })?;
            }
            if request.eof {
                stdin.take();
            }
        }
        let result = DurableStdinEffect {
            request_digest: request.request_digest.to_string(),
            accepted,
        };
        write_new_json(&result_path, &result, "persist sandbox stdin result")?;
        typed(json!({
            "operation_id": operation_id,
            "request_digest": request.request_digest,
            "accepted": accepted,
            "replayed": false,
            "observation": live.observation()?,
        }))
    }

    async fn terminate(&self, target: SandboxTarget) -> EnvironmentResult<SandboxStatus> {
        if target.kind != brain_protocol::environment::TargetKind::Additional
            || target.sandbox_id.is_none()
        {
            return Err(environment_error(
                EnvironmentErrorCode::InvalidRequest,
                false,
                "additional sandbox termination requires an additional target and sandbox_id",
            ));
        }
        let _guard = self.target_gate.lock().await;
        let Some(current) = self.read_physical_target(&target)? else {
            return Err(environment_error(
                EnvironmentErrorCode::SandboxNotMaterialized,
                false,
                "the local additional sandbox was never materialized",
            ));
        };
        if matches!(current.state.as_str(), "gone" | "terminated") {
            return self.status_for(target, &current);
        }
        self.cancel_sandbox_target(&target);
        let terminal = PhysicalTargetState {
            state: "terminated".into(),
            changed_at_ms: brain::wall_ms(),
            ..current
        };
        self.append_physical_target(&target, &terminal)?;
        self.status_for(target, &terminal)
    }
}

#[async_trait]
impl SandboxFilesPort for LocalEnvironment {
    async fn status(&self, target: SandboxTarget) -> EnvironmentResult<SandboxStatus> {
        let _guard = self.target_gate.lock().await;
        let Some(mut current) = self.read_physical_target(&target)? else {
            return typed(json!({
                "target": target,
                "state": "never_materialized",
                "expires_at_ms": null,
            }));
        };
        if !matches!(current.state.as_str(), "gone" | "terminated")
            && current.expires_at_ms <= brain::wall_ms()
        {
            current.state = "gone".into();
            current.changed_at_ms = brain::wall_ms();
            self.append_physical_target(&target, &current)?;
        }
        self.status_for(target, &current)
    }

    async fn list(&self, request: SandboxFileListRequest) -> EnvironmentResult<SandboxFileList> {
        let workspace = self
            .live_workspace(&request.target, &request.expected_generation)
            .await?;
        let base = workspace
            .resolve(&request.path)
            .map_err(brain_error_to_environment)?;
        let mut entries = std::fs::read_dir(&base)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    environment_error(
                        EnvironmentErrorCode::FileNotFound,
                        false,
                        "sandbox directory does not exist",
                    )
                } else {
                    local_io_error("list sandbox directory", error)
                }
            })?
            .filter_map(std::result::Result::ok)
            .map(|entry| {
                let relative = workspace.rel_of(&entry.path());
                Self::file_entry(&workspace, &relative)
            })
            .collect::<EnvironmentResult<Vec<_>>>()?;
        entries.sort_by(|left, right| left.path.as_str().cmp(right.path.as_str()));
        if let Some(cursor) = request.cursor.as_deref() {
            entries.retain(|entry| entry.path.as_str() > cursor);
        }
        let limit = request.limit.clamp(1, 1_000) as usize;
        let next_cursor = (entries.len() > limit).then(|| entries[limit - 1].path.to_string());
        entries.truncate(limit);
        Ok(SandboxFileList {
            entries,
            next_cursor,
        })
    }

    async fn stat(
        &self,
        request: SandboxFileRequest,
    ) -> EnvironmentResult<brain_protocol::environment::FileEntry> {
        let workspace = self
            .live_workspace(&request.target, request.expected_generation.as_str())
            .await?;
        Self::file_entry(&workspace, request.path.as_str())
    }

    async fn read(&self, request: SandboxFileRequest) -> EnvironmentResult<SandboxFileContent> {
        let workspace = self
            .live_workspace(&request.target, request.expected_generation.as_str())
            .await?;
        let entry = Self::file_entry(&workspace, request.path.as_str())?;
        if entry.kind != brain_protocol::environment::FileEntryKind::File {
            return Err(environment_error(
                EnvironmentErrorCode::InvalidRequest,
                false,
                "sandbox read requires a regular file",
            ));
        }
        if entry.bytes > 1024 * 1024 {
            return Err(environment_error(
                EnvironmentErrorCode::ResourceExhausted,
                false,
                "sandbox inline read exceeds 1 MiB",
            ));
        }
        let path = workspace
            .resolve(request.path.as_str())
            .map_err(brain_error_to_environment)?;
        let bytes =
            std::fs::read(path).map_err(|error| local_io_error("read sandbox file", error))?;
        Ok(SandboxFileContent {
            entry,
            content_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        })
    }

    async fn write(&self, request: SandboxFileWriteRequest) -> EnvironmentResult<SandboxFileWriteResult> {
        if brain_protocol::contract::sandbox_file_write_request_digest(&request)
            != request.request_digest
        {
            return Err(environment_error(
                EnvironmentErrorCode::InvalidRequest,
                false,
                "sandbox file write request digest is invalid",
            ));
        }
        let root_id = request.target.root_id.to_string();
        let operation_id = request.operation_id.to_string();
        let request_digest = request.request_digest.to_string();
        let gate = self.operation_gate(&root_id, &operation_id);
        let _guard = gate.lock().await;
        let (effect_dir, replay) =
            self.reserve_file_effect(&root_id, &operation_id, "write", &request_digest)?;
        let result_path = effect_dir.join("write-result.json");
        if replay {
            let mut result = read_json_if_exists::<SandboxFileWriteResult>(
                &result_path,
                "sandbox file write result",
            )?
            .ok_or_else(|| {
                environment_error(
                    EnvironmentErrorCode::OperationUnknown,
                    false,
                    "sandbox file write delivery is ambiguous and will not be repeated",
                )
            })?;
            result.replayed = true;
            return Ok(result);
        }
        let workspace = self
            .live_workspace(&request.target, request.expected_generation.as_str())
            .await?;
        let destination = workspace
            .resolve(request.path.as_str())
            .map_err(brain_error_to_environment)?;
        if destination.exists() && !request.overwrite {
            return Err(environment_error(
                EnvironmentErrorCode::InvalidRequest,
                false,
                "sandbox destination already exists",
            ));
        }
        let bytes = match request.source {
            SandboxFileWriteSource::Inline { content_base64 } => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(content_base64.as_bytes())
                    .map_err(|_| {
                        environment_error(
                            EnvironmentErrorCode::InvalidRequest,
                            false,
                            "sandbox inline content is not valid base64",
                        )
                    })?;
                if bytes.len() > 1024 * 1024 {
                    return Err(environment_error(
                        EnvironmentErrorCode::ResourceExhausted,
                        false,
                        "sandbox inline write exceeds 1 MiB",
                    ));
                }
                bytes
            }
            SandboxFileWriteSource::Object { fetch, object } => {
                let source = Self::transfer_file_path(&fetch, "GET")?;
                let bytes = std::fs::read(source)
                    .map_err(|error| local_io_error("fetch sandbox object", error))?;
                if bytes.len() as u64 != object.bytes
                    || bytes.len() as u64 > fetch.max_bytes.get()
                    || hex::encode(sha2::Sha256::digest(&bytes)) != object.sha256.as_str()
                    || object.object_id != fetch.object_id
                {
                    return Err(environment_error(
                        EnvironmentErrorCode::BindingConflict,
                        false,
                        "sandbox object bytes disagree with their sealed authority",
                    ));
                }
                bytes
            }
        };
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| local_io_error("create sandbox directory", error))?;
        }
        if destination.exists() {
            std::fs::write(&destination, &bytes)
                .map_err(|error| local_io_error("replace sandbox file", error))?;
        } else {
            write_new_bytes(&destination, &bytes, "write sandbox file")?;
        }
        let file = Self::file_entry(&workspace, request.path.as_str())?;
        let result: SandboxFileWriteResult = typed(json!({
            "operation_id": operation_id,
            "request_digest": request_digest,
            "replayed": false,
            "file": file,
        }))?;
        write_new_json(&result_path, &result, "persist sandbox file write result")?;
        Ok(result)
    }

    async fn find(&self, request: SandboxSearchRequest) -> EnvironmentResult<SandboxFileList> {
        let workspace = self
            .live_workspace(&request.target, &request.expected_generation)
            .await?;
        let base = workspace
            .resolve(&request.path)
            .map_err(brain_error_to_environment)?;
        let matcher = globset::GlobBuilder::new(&request.expression)
            .literal_separator(false)
            .build()
            .map_err(|_| {
                environment_error(
                    EnvironmentErrorCode::InvalidRequest,
                    false,
                    "sandbox find expression is not a valid glob",
                )
            })?
            .compile_matcher();
        let mut entries = Vec::new();
        for item in walkdir::WalkDir::new(base)
            .sort_by_file_name()
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            let relative = workspace.rel_of(item.path());
            if matcher.is_match(&relative) {
                entries.push(Self::file_entry(&workspace, &relative)?);
            }
        }
        page_file_entries(entries, request.cursor.as_deref(), request.limit)
    }

    async fn grep(&self, request: SandboxSearchRequest) -> EnvironmentResult<SandboxFileList> {
        let workspace = self
            .live_workspace(&request.target, &request.expected_generation)
            .await?;
        let base = workspace
            .resolve(&request.path)
            .map_err(brain_error_to_environment)?;
        let expression = regex::Regex::new(&request.expression).map_err(|_| {
            environment_error(
                EnvironmentErrorCode::InvalidRequest,
                false,
                "sandbox grep expression is not a valid regular expression",
            )
        })?;
        let mut entries = Vec::new();
        for item in walkdir::WalkDir::new(base)
            .sort_by_file_name()
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            if !item.file_type().is_file()
                || item
                    .metadata()
                    .is_ok_and(|metadata| metadata.len() > 2 * 1024 * 1024)
            {
                continue;
            }
            let Ok(bytes) = std::fs::read(item.path()) else {
                continue;
            };
            if bytes.contains(&0) || !expression.is_match(&String::from_utf8_lossy(&bytes)) {
                continue;
            }
            let relative = workspace.rel_of(item.path());
            entries.push(Self::file_entry(&workspace, &relative)?);
        }
        page_file_entries(entries, request.cursor.as_deref(), request.limit)
    }

    async fn transfer(&self, request: SandboxCopyRequest) -> EnvironmentResult<SandboxCopyResult> {
        if brain_protocol::contract::sandbox_copy_request_digest(&request) != request.request_digest
        {
            return Err(environment_error(
                EnvironmentErrorCode::InvalidRequest,
                false,
                "sandbox copy request digest is invalid",
            ));
        }
        let root_id = request.target.root_id.to_string();
        let operation_id = request.operation_id.to_string();
        let request_digest = request.request_digest.to_string();
        let gate = self.operation_gate(&root_id, &operation_id);
        let _guard = gate.lock().await;
        let (effect_dir, replay) =
            self.reserve_file_effect(&root_id, &operation_id, "copy", &request_digest)?;
        let result_path = effect_dir.join("copy-result.json");
        if replay {
            let mut result =
                read_json_if_exists::<SandboxCopyResult>(&result_path, "sandbox copy result")?
                    .ok_or_else(|| {
                        environment_error(
                            EnvironmentErrorCode::OperationUnknown,
                            false,
                            "sandbox copy delivery is ambiguous and will not be repeated",
                        )
                    })?;
            result.replayed = true;
            return Ok(result);
        }
        let workspace = self
            .live_workspace(&request.target, request.expected_generation.as_str())
            .await?;
        let sandbox_path = workspace
            .resolve(request.path.as_str())
            .map_err(brain_error_to_environment)?;
        let (file, object) = match request.direction {
            SandboxCopyRequestDirection::Import => {
                let object = request.object.as_ref().ok_or_else(|| {
                    environment_error(
                        EnvironmentErrorCode::InvalidRequest,
                        false,
                        "sandbox import requires an object identity",
                    )
                })?;
                let source = Self::transfer_file_path(&request.transfer, "GET")?;
                let bytes = std::fs::read(source)
                    .map_err(|error| local_io_error("import sandbox object", error))?;
                if bytes.len() as u64 != object.bytes
                    || bytes.len() as u64 > request.transfer.max_bytes.get()
                    || object.object_id != request.transfer.object_id
                    || hex::encode(sha2::Sha256::digest(&bytes)) != object.sha256.as_str()
                {
                    return Err(environment_error(
                        EnvironmentErrorCode::BindingConflict,
                        false,
                        "sandbox import bytes disagree with their sealed object",
                    ));
                }
                if sandbox_path.exists() && !request.overwrite {
                    return Err(environment_error(
                        EnvironmentErrorCode::InvalidRequest,
                        false,
                        "sandbox import destination already exists",
                    ));
                }
                if let Some(parent) = sandbox_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|error| {
                        local_io_error("create sandbox import directory", error)
                    })?;
                }
                std::fs::write(&sandbox_path, bytes)
                    .map_err(|error| local_io_error("publish sandbox import", error))?;
                (Self::file_entry(&workspace, request.path.as_str())?, None)
            }
            SandboxCopyRequestDirection::Export => {
                if request.object.is_some() {
                    return Err(environment_error(
                        EnvironmentErrorCode::InvalidRequest,
                        false,
                        "sandbox export must not carry a source object identity",
                    ));
                }
                let destination = Self::transfer_file_path(&request.transfer, "PUT")?;
                let bytes = std::fs::read(&sandbox_path).map_err(|error| {
                    if error.kind() == std::io::ErrorKind::NotFound {
                        environment_error(
                            EnvironmentErrorCode::FileNotFound,
                            false,
                            "sandbox export source does not exist",
                        )
                    } else {
                        local_io_error("read sandbox export", error)
                    }
                })?;
                if bytes.len() as u64 > request.transfer.max_bytes.get() {
                    return Err(environment_error(
                        EnvironmentErrorCode::ResourceExhausted,
                        false,
                        "sandbox export exceeds its sealed byte authority",
                    ));
                }
                if let Some(parent) = destination.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|error| local_io_error("create local upload staging", error))?;
                }
                std::fs::write(&destination, &bytes)
                    .map_err(|error| local_io_error("upload sandbox export", error))?;
                let object: brain_protocol::environment::ObjectReference = typed(json!({
                    "object_id": request.transfer.object_id,
                    "bytes": bytes.len(),
                    "sha256": hex::encode(sha2::Sha256::digest(&bytes)),
                }))?;
                (
                    Self::file_entry(&workspace, request.path.as_str())?,
                    Some(object),
                )
            }
        };
        let result: SandboxCopyResult = typed(json!({
            "operation_id": operation_id,
            "request_digest": request_digest,
            "replayed": false,
            "file": file,
            "object": object,
        }))?;
        write_new_json(&result_path, &result, "persist sandbox copy result")?;
        Ok(result)
    }
}

fn page_file_entries(
    mut entries: Vec<brain_protocol::environment::FileEntry>,
    cursor: Option<&str>,
    limit: u32,
) -> EnvironmentResult<SandboxFileList> {
    entries.sort_by(|left, right| left.path.as_str().cmp(right.path.as_str()));
    if let Some(cursor) = cursor {
        entries.retain(|entry| entry.path.as_str() > cursor);
    }
    let limit = limit.clamp(1, 1_000) as usize;
    let next_cursor = (entries.len() > limit).then(|| entries[limit - 1].path.to_string());
    entries.truncate(limit);
    Ok(SandboxFileList {
        entries,
        next_cursor,
    })
}

fn typed<T: DeserializeOwned>(value: Value) -> EnvironmentResult<T> {
    serde_json::from_value(value).map_err(|_| {
        environment_error(
            EnvironmentErrorCode::InvalidRequest,
            false,
            "local Environment could not construct a valid contract value",
        )
    })
}

fn environment_error(code: EnvironmentErrorCode, retryable: bool, message: &str) -> EnvironmentError {
    serde_json::from_value(json!({
        "code": code,
        "details": {},
        "message": message,
        "retryable": retryable,
    }))
    .expect("static local Environment errors satisfy the contract")
}

fn brain_error_to_environment(error: BrainError) -> EnvironmentError {
    tracing::warn!(error = %error, "local Environment adapter failure");
    environment_error(
        EnvironmentErrorCode::TemporarilyUnavailable,
        true,
        "local Environment storage or execution is temporarily unavailable",
    )
}

fn local_io_error(operation: &str, error: std::io::Error) -> EnvironmentError {
    tracing::warn!(operation, error = %error, "local Environment filesystem failure");
    environment_error(
        EnvironmentErrorCode::TemporarilyUnavailable,
        true,
        "local Environment filesystem is temporarily unavailable",
    )
}

fn hash_component(value: &str) -> String {
    hex::encode(sha2::Sha256::digest(value.as_bytes()))
}

fn read_json<T: DeserializeOwned>(path: &Path, label: &str) -> EnvironmentResult<T> {
    let bytes = std::fs::read(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            environment_error(
                EnvironmentErrorCode::OperationUnknown,
                false,
                "the local operation is unknown",
            )
        } else {
            local_io_error(label, error)
        }
    })?;
    serde_json::from_slice(&bytes).map_err(|_| {
        environment_error(
            EnvironmentErrorCode::TemporarilyUnavailable,
            false,
            "local Environment durable state is invalid",
        )
    })
}

fn read_json_if_exists<T: DeserializeOwned>(path: &Path, label: &str) -> EnvironmentResult<Option<T>> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|_| {
            environment_error(
                EnvironmentErrorCode::TemporarilyUnavailable,
                false,
                "local Environment durable state is invalid",
            )
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(local_io_error(label, error)),
    }
}

fn read_bytes_if_exists(path: &Path, label: &str) -> EnvironmentResult<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(local_io_error(label, error)),
    }
}

fn write_new_json<T: Serialize>(path: &Path, value: &T, label: &str) -> EnvironmentResult<()> {
    let bytes = serde_jcs::to_vec(value).map_err(|_| {
        environment_error(
            EnvironmentErrorCode::InvalidRequest,
            false,
            "local Environment durable value is not canonical JSON",
        )
    })?;
    write_new_bytes(path, &bytes, label)
}

fn write_new_bytes(path: &Path, bytes: &[u8], label: &str) -> EnvironmentResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| local_io_error(label, error))?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(path)
        .map_err(|error| local_io_error(label, error))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| local_io_error(label, error))?;
    if let Some(parent) = path.parent()
        && let Ok(directory) = std::fs::File::open(parent)
    {
        let _ = directory.sync_all();
    }
    Ok(())
}
