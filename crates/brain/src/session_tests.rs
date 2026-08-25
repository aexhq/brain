use super::*;
use crate::provider::fake::{FakeProvider, Scripted};
use crate::storage::SessionStoragePort as _;
use brain_protocol::session::ToolOutcome;
use brain_protocol::session::{ExternalToolCallRequest, ExternalToolCallResponse};
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

fn test_environment_registry(
    extension: &str,
    execution: Arc<dyn crate::environment::EnvironmentPort>,
    preparation: Arc<dyn crate::environment::SessionPreparationPort>,
    files: Option<Arc<dyn crate::environment::SandboxFilesPort>>,
) -> crate::environment::EnvironmentRegistry {
    crate::environment::EnvironmentRegistry::new([(
        extension.to_owned(),
        crate::environment::EnvironmentAdapter {
            execution,
            preparation,
            files,
        },
    )])
    .expect("test environment registry")
}

fn declare_test_managed_environment(head: &mut HeadDoc, tool_name: &str, bundle_digest: &str) {
    head.prefix.environments.insert(
        "workspace".into(),
        serde_json::from_value(json!({
            "extension":"test.managed",
            "protocol":"environment/v1",
            "profile":{
                "kind":"computer",
                "platform":"linux-amd64",
                "network":"allowlist",
                "recovery":"retained"
            },
            "configuration":{}
        }))
        .expect("valid test environment declaration"),
    );
    head.prefix.managed_bundles.push(
        serde_json::from_value(json!({
            "bundle_digest":bundle_digest,
            "bytes":1,
            "contract_digest":"b".repeat(64),
            "layers":[{
                "digest":bundle_digest,
                "bytes":1,
                "media_type":"application/javascript+esm",
                "mount_path":"/tool/runtime.mjs",
                "unpack":"file",
                "object":{
                    "bytes":1,
                    "media_type":"application/javascript+esm",
                    "object_id":format!("bundle_{bundle_digest}"),
                    "sha256":bundle_digest
                }
            }],
            "required_env":[],
            "target":"linux-amd64",
            "execute_path":"/tool/runtime.mjs",
            "setup_path":null,
            "environment_name":"workspace",
            "tool_name":tool_name
        }))
        .expect("valid test managed bundle descriptor"),
    );
}

fn typed_create(value: serde_json::Value) -> CreateSessionRequest {
    typed_create_result(value).expect("test CreateSessionRequest deserializes")
}

fn typed_create_result(mut value: serde_json::Value) -> serde_json::Result<CreateSessionRequest> {
    let object = value
        .as_object_mut()
        .expect("test create request is an object");
    object.remove("system_prompt");
    let model_component = b"test model";
    let model_digest = hex::encode(Sha256::digest(model_component));
    if let Some(model) = object
        .get_mut("model")
        .and_then(serde_json::Value::as_object_mut)
    {
        model
            .entry("component_digest")
            .or_insert_with(|| json!(model_digest.clone()));
        model
            .entry("world")
            .or_insert_with(|| json!("aex:model/model@1.0.0"));
    }
    let loop_component = b"test loop";
    let loop_digest = hex::encode(Sha256::digest(loop_component));
    object.entry("agentloop").or_insert_with(|| {
        json!({
            "component_digest": loop_digest.clone(),
            "world": "aex:agentloop/agentloop@1.0.0"
        })
    });
    object.entry("component_artifacts").or_insert_with(|| json!([
        {
            "component_digest": model_digest,
            "component_base64": base64::engine::general_purpose::STANDARD.encode(model_component),
            "bytes": model_component.len()
        },
        {
            "component_digest": loop_digest,
            "component_base64": base64::engine::general_purpose::STANDARD.encode(loop_component),
            "bytes": loop_component.len()
        }
    ]));
    serde_json::from_value(value)
}

#[tokio::test]
async fn tenant_changefeed_is_partitioned_paginated_and_tenant_scoped() {
    let brain = Brain::with_parts_and_services(
        BrainConfig::default(),
        Journal::new_memory("brain-changefeed"),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices::default(),
        crate::provider::fake::unscripted_factory(),
    );
    let tenant = TrustedPrincipal::new("tenant-a").unwrap();
    let other = TrustedPrincipal::new("tenant-b").unwrap();
    let create = || {
        typed_create(json!({
            "model": {"provider":"anthropic", "name":"model", "api_key":"key"}
        }))
    };
    let mut expected = std::collections::HashSet::new();
    for key in ["one", "two", "three"] {
        let session = brain
            .create_session_for(&tenant, create(), Some(key))
            .await
            .unwrap();
        expected.insert(session.id.to_string());
    }
    brain
        .create_session_for(&other, create(), Some("other"))
        .await
        .unwrap();

    let (first, cursor, first_watermark) = brain
        .list_changes_for(&tenant, 0, 0, 1, 2, None)
        .await
        .unwrap();
    assert_eq!(first.len(), 2);
    let cursor = cursor.expect("the remaining tenant session requires another page");
    let (second, cursor, second_watermark) = brain
        .list_changes_for(&tenant, 0, 0, 1, 2, Some(&cursor))
        .await
        .unwrap();
    assert_eq!(second.len(), 1);
    assert!(cursor.is_none());
    assert!(first_watermark > 0);
    assert!(second_watermark > 0);
    let actual = first
        .into_iter()
        .chain(second)
        .map(|session| session.id.to_string())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(actual, expected);

    let (none, cursor, watermark) = brain
        .list_changes_for(&tenant, u64::MAX, 0, 1, 100, None)
        .await
        .unwrap();
    assert!(none.is_empty());
    assert!(cursor.is_none());
    assert_eq!(watermark, u64::MAX);
    assert!(
        brain
            .list_changes_for(&tenant, 0, 1, 1, 100, None)
            .await
            .is_err()
    );
}

struct ReleaseTrackingEnvironment {
    releases: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl crate::environment::ComponentEnvironmentRegistry for ReleaseTrackingEnvironment {
    fn admit(&self, _component_digest: &str, _world: &str, _component: &[u8]) -> Result<()> {
        Ok(())
    }

    async fn invoke(
        &self,
        _declaration: &brain_protocol::session::ComponentEnvironmentConfig,
        _request: crate::environment::ComponentEnvironmentInvocation,
    ) -> Result<String> {
        Err(BrainError::Environment("unexpected test invocation".into()))
    }

    async fn release(
        &self,
        _declaration: &brain_protocol::session::ComponentEnvironmentConfig,
        request: crate::environment::ComponentEnvironmentRelease,
    ) -> Result<()> {
        assert_eq!(request.session_id, request.root_id);
        assert!(request.parent_id.is_none());
        assert_eq!(request.environment_id, "workspace");
        self.releases.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[tokio::test]
async fn ending_a_root_releases_each_component_environment() {
    let releases = Arc::new(AtomicUsize::new(0));
    let registry = Arc::new(ReleaseTrackingEnvironment {
        releases: releases.clone(),
    });
    let brain = Brain::with_parts_and_services(
        BrainConfig::default(),
        Journal::new_memory("brain-component-environment-release"),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices {
            component_environment_registry: Some(registry),
            ..BrainServices::default()
        },
        crate::provider::fake::unscripted_factory(),
    );
    let component = b"test environment";
    let digest = hex::encode(Sha256::digest(component));
    let mut request = typed_create(json!({
        "model": {"provider":"anthropic", "name":"model", "api_key":"key"},
        "environments": {"workspace": {
            "component_digest": digest,
            "world": crate::environment::COMPONENT_ENVIRONMENT_WORLD,
            "config": {}
        }}
    }));
    request.component_artifacts.push(
        serde_json::from_value(json!({
            "component_digest": hex::encode(Sha256::digest(component)),
            "component_base64": base64::engine::general_purpose::STANDARD.encode(component),
            "bytes": component.len()
        }))
        .unwrap(),
    );
    let created = brain.create_session(request, None).await.unwrap();
    brain.end(created.id.as_str()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while releases.load(Ordering::Relaxed) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("component Environment release");
    assert_eq!(releases.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn suspending_a_root_releases_component_environments_until_resume() {
    let releases = Arc::new(AtomicUsize::new(0));
    let registry = Arc::new(ReleaseTrackingEnvironment {
        releases: releases.clone(),
    });
    let journal = Journal::new_memory("brain-component-environment-suspend");
    let brain = Brain::with_parts_and_services(
        BrainConfig::default(),
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices {
            component_environment_registry: Some(registry),
            ..BrainServices::default()
        },
        crate::provider::fake::unscripted_factory(),
    );
    let component = b"test environment";
    let digest = hex::encode(Sha256::digest(component));
    let mut request = typed_create(json!({
        "model": {"provider":"anthropic", "name":"model", "api_key":"key"},
        "environments": {"workspace": {
            "component_digest": digest,
            "world": crate::environment::COMPONENT_ENVIRONMENT_WORLD,
            "config": {}
        }}
    }));
    request.component_artifacts.push(
        serde_json::from_value(json!({
            "component_digest": hex::encode(Sha256::digest(component)),
            "component_base64": base64::engine::general_purpose::STANDARD.encode(component),
            "bytes": component.len()
        }))
        .unwrap(),
    );
    let created = brain.create_session(request, None).await.unwrap();
    let root_id = created.id.to_string();
    let mut child = journal.get_head(&root_id).await.unwrap().doc;
    let child_id = "ses_suspendchild000000000";
    child.parent_id = Some(root_id.clone());
    child.ancestor_ids = vec![root_id.clone()];
    child.depth = 1;
    child.create_key_hash = None;
    child.create_request_hash = None;
    journal
        .create(
            child_id,
            &child,
            &Record::State {
                state: SessionLifecycle::Open,
                turn: None,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        brain.suspend(child_id).await,
        Err(BrainError::Invalid(message)) if message.contains("root session")
    ));

    let suspended = brain.suspend(created.id.as_str()).await.unwrap();
    assert_eq!(suspended.state, session::SessionState::Suspended);
    assert_eq!(releases.load(Ordering::Relaxed), 1);
    assert!(matches!(
        brain
            .message(
                created.id.as_str(),
                MessageRequestContent::String("blocked".parse().unwrap()),
            )
            .await,
        Err(BrainError::Invalid(message)) if message.contains("resume")
    ));

    let resumed = brain.resume(created.id.as_str()).await.unwrap();
    assert_eq!(resumed.state, session::SessionState::Open);
    assert_eq!(releases.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn durable_retention_is_renewable_and_shortening_is_explicit() {
    let brain = Brain::with_parts_and_services(
        BrainConfig::default(),
        Journal::new_memory("brain-retention-update"),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices::default(),
        crate::provider::fake::unscripted_factory(),
    );
    let created = brain
        .create_session(
            typed_create(json!({
                "model": {"provider":"anthropic", "name":"model", "api_key":"key"}
            })),
            None,
        )
        .await
        .unwrap();
    let initial = u64::try_from(created.retain_until.timestamp_millis()).unwrap();
    let extended = initial + 60_000;
    let renewed = brain
        .update_retention(created.id.as_str(), extended, false)
        .await
        .unwrap();
    assert_eq!(
        u64::try_from(renewed.retain_until.timestamp_millis()).unwrap(),
        extended
    );

    let shortened = initial - 60_000;
    assert!(matches!(
        brain
            .update_retention(created.id.as_str(), shortened, false)
            .await,
        Err(BrainError::Invalid(message)) if message.contains("allow_shorten")
    ));
    let updated = brain
        .update_retention(created.id.as_str(), shortened, true)
        .await
        .unwrap();
    assert_eq!(
        u64::try_from(updated.retain_until.timestamp_millis()).unwrap(),
        shortened
    );
}

#[tokio::test]
async fn expired_durable_retention_reuses_the_recoverable_deletion_path() {
    let journal = Journal::new_memory("brain-retention-expiry");
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            default_retention: Duration::from_millis(40),
            max_retention: Duration::from_secs(1),
            recovery_poll_interval: Duration::from_millis(5),
            recovery_shards_per_poll: crate::journal::RECOVERY_SHARDS,
            ..BrainConfig::default()
        },
        journal,
        Arc::new(crate::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices::default(),
        crate::provider::fake::unscripted_factory(),
    );
    brain.start_recovery_worker();
    let created = brain
        .create_session(
            typed_create(json!({
                "model": {"provider":"anthropic", "name":"model", "api_key":"key"}
            })),
            None,
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if brain
                .deletion_status(created.id.as_str())
                .await
                .is_ok_and(|status| status.state == DeletionState::Succeeded)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("expired session reaches the durable deletion tombstone");
    assert!(matches!(
        brain.get(created.id.as_str()).await,
        Err(BrainError::NoSuchSession(_))
    ));
}

#[test]
fn journal_retention_config_rejects_malformed_and_inconsistent_policy() {
    assert_eq!(parse_strict_env_u64("TEST_LIMIT", None, 17).unwrap(), 17);
    assert_eq!(
        parse_strict_env_u64("TEST_LIMIT", Some("42"), 17).unwrap(),
        42
    );
    for raw in ["", "-1", "1.5", "not-a-number"] {
        assert!(matches!(
            parse_strict_env_u64("TEST_LIMIT", Some(raw), 17),
            Err(BrainError::Invalid(_))
        ));
    }

    let mut cfg = BrainConfig::default();
    cfg.journal_max_session_bytes = crate::journal::MIN_SESSION_JOURNAL_BYTES;
    cfg.journal_max_tenant_bytes = cfg.journal_max_session_bytes;
    cfg.journal_max_tenant_sessions = 1;
    cfg.validate().unwrap();

    cfg.journal_max_tenant_bytes = cfg.journal_max_session_bytes - 1;
    assert!(matches!(cfg.validate(), Err(BrainError::Invalid(_))));
    cfg.journal_max_tenant_bytes = cfg.journal_max_session_bytes;
    cfg.journal_max_tenant_sessions = 0;
    assert!(matches!(cfg.validate(), Err(BrainError::Invalid(_))));
}

#[test]
fn process_environment_policy_uses_exact_defaults_and_bounds() {
    let cfg = BrainConfig::from_env_values(|_| Ok(None)).unwrap();
    assert_eq!(
        cfg.max_concurrent_model_rounds,
        DEFAULT_MAX_CONCURRENT_MODEL_ROUNDS
    );
    assert_eq!(cfg.max_concurrent_turns, DEFAULT_MAX_CONCURRENT_TURNS);
    assert_eq!(cfg.max_concurrent_creates, DEFAULT_MAX_CONCURRENT_CREATES);
    assert_eq!(cfg.max_event_followers, DEFAULT_MAX_EVENT_FOLLOWERS);
    assert_eq!(cfg.max_resident_sessions, DEFAULT_MAX_RESIDENT_SESSIONS);
    assert_eq!(
        cfg.default_retention,
        Duration::from_secs(DEFAULT_RETENTION_SECONDS)
    );
    assert_eq!(
        cfg.max_retention,
        Duration::from_secs(MAX_RETENTION_SECONDS)
    );
    assert_eq!(
        cfg.storage_max_tenant_bytes,
        DEFAULT_STORAGE_MAX_TENANT_BYTES
    );
    assert_eq!(
        cfg.max_concurrent_recoveries,
        DEFAULT_MAX_CONCURRENT_RECOVERIES
    );

    for (name, default, minimum, maximum) in [
        (
            MAX_MODEL_ROUNDS_ENV,
            DEFAULT_MAX_CONCURRENT_MODEL_ROUNDS,
            1,
            MAX_CONCURRENT_MODEL_ROUNDS,
        ),
        (
            MAX_TURNS_ENV,
            DEFAULT_MAX_CONCURRENT_TURNS,
            1,
            MAX_CONCURRENT_TURNS,
        ),
        (
            MAX_CONCURRENT_CREATES_ENV,
            DEFAULT_MAX_CONCURRENT_CREATES,
            1,
            MAX_CONCURRENT_CREATES,
        ),
        (
            MAX_EVENT_FOLLOWERS_ENV,
            DEFAULT_MAX_EVENT_FOLLOWERS,
            1,
            MAX_EVENT_FOLLOWERS,
        ),
        (
            MAX_RESIDENT_SESSIONS_ENV,
            DEFAULT_MAX_RESIDENT_SESSIONS,
            1,
            MAX_RESIDENT_SESSIONS,
        ),
        (
            RECOVERY_SHARDS_PER_POLL_ENV,
            DEFAULT_RECOVERY_SHARDS_PER_POLL,
            1,
            crate::journal::RECOVERY_SHARDS,
        ),
        (
            RECOVERY_PAGE_SIZE_ENV,
            DEFAULT_RECOVERY_PAGE_SIZE,
            1,
            MAX_RECOVERY_PAGE_SIZE,
        ),
        (
            MAX_CONCURRENT_RECOVERIES_ENV,
            DEFAULT_MAX_CONCURRENT_RECOVERIES,
            1,
            MAX_CONCURRENT_RECOVERIES,
        ),
    ] {
        assert_eq!(
            parse_env_usize(name, None, default, minimum, maximum).unwrap(),
            default
        );
        assert_eq!(
            parse_env_usize(name, Some(&minimum.to_string()), default, minimum, maximum).unwrap(),
            minimum
        );
        assert_eq!(
            parse_env_usize(name, Some(&maximum.to_string()), default, minimum, maximum).unwrap(),
            maximum
        );
        assert!(
            parse_env_usize(
                name,
                Some(&maximum.saturating_add(1).to_string()),
                default,
                minimum,
                maximum,
            )
            .is_err()
        );
        for invalid in ["", "-1", "1.5", "not-a-number"] {
            assert!(
                parse_env_usize(name, Some(invalid), default, minimum, maximum).is_err(),
                "{name} accepted {invalid:?}"
            );
        }
    }

    for (name, default, minimum, maximum) in [
        (
            PROVIDER_HEADER_TIMEOUT_ENV,
            DEFAULT_PROVIDER_HEADER_TIMEOUT_MS,
            MIN_PROVIDER_HEADER_TIMEOUT_MS,
            MAX_PROVIDER_HEADER_TIMEOUT_MS,
        ),
        (
            PROVIDER_IDLE_TIMEOUT_ENV,
            DEFAULT_PROVIDER_IDLE_TIMEOUT_MS,
            MIN_PROVIDER_IDLE_TIMEOUT_MS,
            MAX_PROVIDER_IDLE_TIMEOUT_MS,
        ),
        (
            PROVIDER_TOTAL_TIMEOUT_ENV,
            DEFAULT_PROVIDER_TOTAL_TIMEOUT_MS,
            MIN_PROVIDER_TOTAL_TIMEOUT_MS,
            MAX_PROVIDER_TOTAL_TIMEOUT_MS,
        ),
        (
            EXTERNAL_TOOL_TIMEOUT_ENV,
            DEFAULT_EXTERNAL_TOOL_TIMEOUT_MS,
            MIN_EXTERNAL_TOOL_TIMEOUT_MS,
            MAX_EXTERNAL_TOOL_TIMEOUT_MS,
        ),
        (
            STORAGE_TRANSFER_TTL_ENV,
            crate::storage::DEFAULT_STORAGE_TRANSFER_TTL_MS,
            MIN_STORAGE_TRANSFER_TTL_MS,
            MAX_STORAGE_TRANSFER_TTL_MS,
        ),
        (
            RECOVERY_POLL_ENV,
            DEFAULT_RECOVERY_POLL_MS,
            MIN_RECOVERY_POLL_MS,
            MAX_RECOVERY_POLL_MS,
        ),
    ] {
        assert_eq!(
            parse_env_u64(name, None, default, minimum, maximum).unwrap(),
            default
        );
        assert_eq!(
            parse_env_u64(name, Some(&minimum.to_string()), default, minimum, maximum).unwrap(),
            minimum
        );
        assert_eq!(
            parse_env_u64(name, Some(&maximum.to_string()), default, minimum, maximum).unwrap(),
            maximum
        );
        assert!(
            parse_env_u64(
                name,
                Some(&maximum.saturating_add(1).to_string()),
                default,
                minimum,
                maximum,
            )
            .is_err()
        );
    }
}

#[test]
fn process_environment_policy_rejects_cross_field_and_string_drift() {
    let load = |values: &[(&str, &str)]| {
        BrainConfig::from_env_values(|name| {
            Ok(values
                .iter()
                .find_map(|(candidate, value)| (*candidate == name).then(|| (*value).into())))
        })
    };

    assert!(load(&[(MAX_TURNS_ENV, "0")]).is_err());
    assert!(load(&[(OUTBOUND_ALLOW_PRIVATE_ENV, "TRUE")]).is_err());
    assert!(
        load(&[
            (DEFAULT_RETENTION_SECONDS_ENV, "120"),
            (MAX_RETENTION_SECONDS_ENV, "60"),
        ])
        .is_err()
    );
    assert!(
        load(&[
            (STORAGE_MAX_OBJECT_BYTES_ENV, "2"),
            (STORAGE_MAX_SESSION_BYTES_ENV, "1"),
        ])
        .is_err()
    );
    assert!(
        load(&[
            (PROVIDER_HEADER_TIMEOUT_ENV, "3000"),
            (PROVIDER_TOTAL_TIMEOUT_ENV, "2000"),
        ])
        .is_err()
    );
    assert!(load(&[(EXTERNAL_EXECUTOR_TOKEN_ENV, "secret-without-url")]).is_err());
    assert!(
        load(&[(
            EXTERNAL_EXECUTOR_URL_ENV,
            "http://127.0.0.1:1234/tools?credential=sentinel",
        )])
        .is_err()
    );
    assert!(
        load(&[
            (EXTERNAL_EXECUTOR_URL_ENV, "http://127.0.0.1:1234/tools"),
            (EXTERNAL_EXECUTOR_TOKEN_ENV, "invalid\nheader"),
        ])
        .is_err()
    );
    assert!(
        load(&[
            (EXTERNAL_EXECUTOR_URL_ENV, "http://127.0.0.1:1234/tools"),
            (
                EXTERNAL_EXECUTOR_POLICIES_ENV,
                r#"[{"capability":"brain.output","scope":"root","completion":"return_direct","effect":"replay_safe","max_input_bytes":1024},{"capability":"brain.output","scope":"all","completion":"continue","effect":"replay_safe","max_input_bytes":1024}]"#
            ),
        ])
        .is_err()
    );
    load(&[
        (EXTERNAL_EXECUTOR_URL_ENV, "http://127.0.0.1:1234/tools"),
        (EXTERNAL_EXECUTOR_TOKEN_ENV, "valid-token"),
        (
            EXTERNAL_EXECUTOR_POLICIES_ENV,
            r#"[{"capability":"brain.output","scope":"root","completion":"return_direct","effect":"replay_safe","max_input_bytes":1024},{"capability":"brain.web","scope":"all","completion":"continue","effect":"replay_safe","max_input_bytes":8192}]"#,
        ),
    ])
    .unwrap();
}

#[test]
fn transport_timeout_policy_accepts_exact_bounds_and_rejects_invalid_order() {
    let mut cfg = BrainConfig {
        provider_header_timeout: Duration::from_millis(MIN_PROVIDER_HEADER_TIMEOUT_MS),
        provider_idle_timeout: Duration::from_millis(MIN_PROVIDER_IDLE_TIMEOUT_MS),
        provider_total_timeout: Duration::from_millis(MIN_PROVIDER_TOTAL_TIMEOUT_MS),
        external_call_timeout: Duration::from_millis(MIN_EXTERNAL_TOOL_TIMEOUT_MS),
        ..BrainConfig::default()
    };
    cfg.validate().unwrap();

    cfg.provider_header_timeout = Duration::from_millis(MAX_PROVIDER_HEADER_TIMEOUT_MS);
    cfg.provider_idle_timeout = Duration::from_millis(MAX_PROVIDER_IDLE_TIMEOUT_MS);
    cfg.provider_total_timeout = Duration::from_millis(MAX_PROVIDER_TOTAL_TIMEOUT_MS);
    cfg.external_call_timeout = Duration::from_millis(MAX_EXTERNAL_TOOL_TIMEOUT_MS);
    cfg.validate().unwrap();

    cfg.provider_header_timeout =
        Duration::from_millis(MIN_PROVIDER_HEADER_TIMEOUT_MS.saturating_sub(1));
    assert!(matches!(cfg.validate(), Err(BrainError::Invalid(_))));
    cfg.provider_header_timeout = Duration::from_millis(MAX_PROVIDER_HEADER_TIMEOUT_MS);
    cfg.provider_idle_timeout =
        Duration::from_millis(MAX_PROVIDER_IDLE_TIMEOUT_MS.saturating_add(1));
    assert!(matches!(cfg.validate(), Err(BrainError::Invalid(_))));
    cfg.provider_idle_timeout = Duration::from_millis(MAX_PROVIDER_IDLE_TIMEOUT_MS);
    cfg.external_call_timeout =
        Duration::from_millis(MAX_EXTERNAL_TOOL_TIMEOUT_MS.saturating_add(1));
    assert!(matches!(cfg.validate(), Err(BrainError::Invalid(_))));

    cfg.external_call_timeout = Duration::from_millis(DEFAULT_EXTERNAL_TOOL_TIMEOUT_MS);
    cfg.provider_header_timeout = Duration::from_millis(DEFAULT_PROVIDER_HEADER_TIMEOUT_MS);
    cfg.provider_idle_timeout = Duration::from_millis(DEFAULT_PROVIDER_IDLE_TIMEOUT_MS);
    cfg.provider_total_timeout =
        Duration::from_millis(DEFAULT_PROVIDER_IDLE_TIMEOUT_MS.saturating_sub(1));
    assert!(matches!(cfg.validate(), Err(BrainError::Invalid(_))));
}

#[tokio::test]
async fn resident_actor_admission_is_hard_bounded_and_prunes_dead_root_cells() {
    let data_dir = std::env::temp_dir().join(format!(
        "brain-resident-cap-{}-{}",
        std::process::id(),
        crate::wall_ms()
    ));
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            max_resident_sessions: 2,
            idle_discard: Duration::from_millis(100),
            ..BrainConfig::default()
        },
        Journal::new_memory("brain-resident-cap"),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices::default(),
        crate::provider::fake::unscripted_factory(),
    );
    let first = brain
        .spawn_actor("ses_resident_1", ActorStartup::Lazy)
        .await
        .unwrap();
    let second = brain
        .spawn_actor("ses_resident_2", ActorStartup::Lazy)
        .await
        .unwrap();
    let third = brain
        .spawn_actor("ses_resident_3", ActorStartup::Lazy)
        .await
        .expect("pressure evicts one safe idle resident");
    assert!(brain.sessions.lock().expect("sessions").len() <= 2);

    {
        let mut cells = brain.root_secret_cells.lock().expect("root secret cells");
        for index in 0..1_000 {
            cells.insert(format!("root-{index}"), Weak::new());
        }
    }

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if first.is_closed()
                && second.is_closed()
                && brain.sessions.lock().expect("sessions").is_empty()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("idle residents release their slots");
    assert!(
        brain
            .root_secret_cells
            .lock()
            .expect("root secret cells")
            .is_empty()
    );

    let fourth = brain
        .spawn_actor("ses_resident_4", ActorStartup::Lazy)
        .await
        .expect("released resident capacity is immediately reusable");
    assert!(!third.is_closed() || !fourth.is_closed());
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn resident_pressure_returns_overload_when_every_slot_is_active() {
    let data_dir = std::env::temp_dir().join(format!(
        "brain-resident-active-cap-{}-{}",
        std::process::id(),
        crate::wall_ms()
    ));
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            max_resident_sessions: 1,
            ..BrainConfig::default()
        },
        Journal::new_memory("brain-resident-active-cap"),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices::default(),
        crate::provider::fake::unscripted_factory(),
    );
    // The resident permit is the authoritative process bound and remains held across every
    // active turn/effect. With no idle actor registered for pressure, admission waits once
    // for the fixed short window and fails honestly.
    let _active = brain.resident_permits.clone().try_acquire_owned().unwrap();
    let started = std::time::Instant::now();
    assert!(matches!(
        brain
            .spawn_actor("ses_resident_busy", ActorStartup::Lazy)
            .await,
        Err(BrainError::Overloaded)
    ));
    assert!(started.elapsed() >= Duration::from_millis(200));
    assert!(brain.sessions.lock().expect("sessions").is_empty());
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn end_returns_the_durable_fence_before_async_teardown_converges() {
    let data_dir = std::env::temp_dir().join(format!(
        "brain-async-end-{}-{}",
        std::process::id(),
        crate::wall_ms()
    ));
    let journal = Journal::new_memory("brain-async-end");
    let brain = Brain::with_parts_and_services(
        BrainConfig::default(),
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices::default(),
        crate::provider::fake::unscripted_factory(),
    );
    let created = brain
        .create_session(
            typed_create(json!({
                "model": {"provider":"anthropic", "name":"model", "api_key":"key"}
            })),
            Some("async-end"),
        )
        .await
        .unwrap();
    let session_id = created.id.to_string();
    assert_eq!(
        created.model.context_window_tokens,
        i64::from(brain_protocol::DEFAULT_MODEL_CONTEXT_WINDOW_TOKENS)
    );

    let accepted = brain.end(&session_id).await.unwrap();
    assert_eq!(accepted.state, session::SessionState::Ending);
    assert_eq!(accepted.turn_state, session::SessionTurnState::Idle);
    assert!(
        journal.get_head(&session_id).await.unwrap().doc.ended,
        "the response must be backed by the durable admission fence"
    );

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if journal.get_head(&session_id).await.unwrap().doc.state == SessionLifecycle::Ended {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("background end convergence");
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn end_fences_before_a_cancellation_resistant_effect_and_recovery_never_reopens() {
    let data_dir = std::env::temp_dir().join(format!(
        "brain-end-resistant-effect-{}-{}",
        std::process::id(),
        crate::wall_ms()
    ));
    std::fs::create_dir_all(&data_dir).expect("create resistant effect data dir");
    let journal = Journal::new_memory("brain-end-resistant-effect");
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script([Scripted::tool("submit", json!({"answer": 42}))]);
    let provider = fake.clone();
    let executor = Arc::new(CancellationResistantExecutor::default());
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            idle_discard: Duration::from_secs(300),
            official_capabilities: HashMap::from([("brain.submit".into(), submit_policy())]),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        executor.clone(),
        BrainServices::default(),
        Arc::new(move |_| provider.clone()),
    );
    let root = brain
        .create_session(
            typed_create(json!({
                "model": {
                    "provider":"anthropic", "name":"resistant-effect", "api_key":"key"
                },
                "tools": {"items": [{
                    "definition": {
                        "name":"submit", "description":"submit a final result",
                        "contract_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "input_schema":{"type":"object"},
                        "output_schema":{"type":"object"}
                    },
                    "executor":{"kind":"engine", "capability":"brain.submit"}
                }]}
            })),
            Some("resistant-end"),
        )
        .await
        .expect("create resistant-effect root");
    let root_id = root.id.to_string();

    // Persist a depth-two descendant so admission has to evaluate the immutable ancestor
    // chain while the root actor itself is parked inside the resistant effect.
    let root_head = journal.get_head(&root_id).await.expect("root head");
    let child_id = "ses_resistantendchild0000";
    let mut child = root_head.doc.clone();
    child.root_id = root_id.clone();
    child.parent_id = Some(root_id.clone());
    child.ancestor_ids = vec![root_id.clone()];
    child.depth = 1;
    child.last_seq = 1;
    child.create_key_hash = None;
    child.create_request_hash = None;
    child.context_fork = None;
    journal
        .create(
            child_id,
            &child,
            &Record::State {
                state: SessionLifecycle::Open,
                turn: None,
            },
        )
        .await
        .expect("create resistant-effect child");
    let grandchild_id = "ses_resistantendgrand0000";
    let mut grandchild = child.clone();
    grandchild.parent_id = Some(child_id.into());
    grandchild.ancestor_ids = vec![root_id.clone(), child_id.into()];
    grandchild.depth = 2;
    journal
        .create(
            grandchild_id,
            &grandchild,
            &Record::State {
                state: SessionLifecycle::Open,
                turn: None,
            },
        )
        .await
        .expect("create resistant-effect grandchild");

    brain
        .message(
            &root_id,
            MessageRequestContent::String("run the resistant effect".parse().unwrap()),
        )
        .await
        .expect("admit root turn");
    tokio::time::timeout(Duration::from_secs(2), async {
        while executor.calls.load(Ordering::Acquire) == 0 {
            executor.entered.notified().await;
        }
    })
    .await
    .expect("external effect starts");

    let accepted = tokio::time::timeout(Duration::from_secs(1), brain.end(&root_id))
        .await
        .expect("END acceptance cannot wait for a cancellation-resistant effect")
        .expect("END fence commits");
    assert_eq!(accepted.state, session::SessionState::Ending);
    assert!(!executor.released.load(Ordering::Acquire));
    let fenced = journal.get_head(&root_id).await.expect("fenced root");
    assert_eq!(fenced.doc.state, SessionLifecycle::Ending);
    assert!(fenced.doc.ended);
    assert!(
        fenced.doc.turn.is_some(),
        "pending work remains recoverable"
    );

    let descendant_error = brain
        .message(
            grandchild_id,
            MessageRequestContent::String("late follow-up".parse().unwrap()),
        )
        .await
        .expect_err("the durable ancestor fence closes deep admission immediately");
    assert!(matches!(descendant_error, BrainError::Fenced));

    executor.release();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let head = journal.get_head(&root_id).await.unwrap();
            if head.doc.turn.is_none() && executor.calls.load(Ordering::Acquire) >= 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("a new owner reconciles the resistant effect after the fence");
    assert!(matches!(
        journal.get_head(&root_id).await.unwrap().doc.state.as_str(),
        "ending" | "ended"
    ));
    let records = journal
        .read_records(&root_id, 0)
        .await
        .expect("root records");
    let ending_seq = records
        .iter()
        .find(|entry| matches!(&entry.record, Record::State { state, .. } if *state == SessionLifecycle::Ending))
        .expect("durable ending record")
        .seq;
    assert!(
        records.iter().all(|entry| {
            entry.seq <= ending_seq
                || !matches!(&entry.record, Record::State { state, .. } if *state == SessionLifecycle::Open)
        }),
        "turn reconciliation after the END fence must never reopen the lifecycle"
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
fn complete_create_contract_bounds_are_enforced_before_resolution() {
    let omitted = typed_create(json!({
        "model": {"provider":"anthropic", "name":"model", "api_key":"key"}
    }));
    validate_create_request(&omitted).expect("omitted values use schema defaults");

    let exact = typed_create(json!({
        "model": {
            "provider":"anthropic", "name":"model", "api_key":"key",
            "max_output_tokens": brain_protocol::MAX_MODEL_OUTPUT_TOKENS,
            "context_window_tokens": brain_protocol::MAX_MODEL_CONTEXT_WINDOW_TOKENS,
            "temperature": 2.0
        },
        "provider_recovery_retries": 8,
        "client": {"id":"app", "submit_retries":8},
        "children": {
            "max_depth":8, "max_direct_children":128,
            "max_descendants":1024
        }
    }));
    validate_create_request(&exact).expect("every exact public maximum is accepted");

    for (label, value) in [
        (
            "provider_recovery_retries",
            json!({"model":{"provider":"anthropic","name":"model","api_key":"key"},"provider_recovery_retries":9}),
        ),
        (
            "client.submit_retries",
            json!({"model":{"provider":"anthropic","name":"model","api_key":"key"},"client":{"id":"app","submit_retries":9}}),
        ),
        (
            "children.max_depth",
            json!({"model":{"provider":"anthropic","name":"model","api_key":"key"},"children":{"max_depth":9}}),
        ),
        (
            "children.max_direct_children",
            json!({"model":{"provider":"anthropic","name":"model","api_key":"key"},"children":{"max_direct_children":129}}),
        ),
        (
            "children.max_descendants",
            json!({"model":{"provider":"anthropic","name":"model","api_key":"key"},"children":{"max_descendants":1025}}),
        ),
        (
            "model.max_output_tokens",
            json!({"model":{"provider":"anthropic","name":"model","api_key":"key","max_output_tokens":u64::from(brain_protocol::MAX_MODEL_OUTPUT_TOKENS)+1}}),
        ),
        (
            "model.context_window_tokens below minimum",
            json!({"model":{"provider":"anthropic","name":"model","api_key":"key","context_window_tokens":i64::from(brain_protocol::MIN_MODEL_CONTEXT_WINDOW_TOKENS)-1}}),
        ),
        (
            "model.context_window_tokens above maximum",
            json!({"model":{"provider":"anthropic","name":"model","api_key":"key","context_window_tokens":i64::from(brain_protocol::MAX_MODEL_CONTEXT_WINDOW_TOKENS)+1}}),
        ),
        (
            "model.temperature",
            json!({"model":{"provider":"anthropic","name":"model","api_key":"key","temperature":2.01}}),
        ),
    ] {
        let request = typed_create(value);
        let error = validate_create_request(&request)
            .expect_err("a value above the public maximum must be rejected");
        assert!(
            matches!(error, BrainError::Invalid(_)),
            "{label} produced {error:?}"
        );
    }

    let secrets = (0..129)
        .map(|index| (format!("SECRET_{index}"), json!("value")))
        .collect::<serde_json::Map<_, _>>();
    let request = typed_create(json!({
        "model":{"provider":"anthropic","name":"model","api_key":"key"},
        "secrets": secrets
    }));
    assert!(matches!(
        validate_create_request(&request),
        Err(BrainError::Invalid(_))
    ));

    let exact_secret_document = typed_create(json!({
        "model":{"provider":"anthropic","name":"model","api_key":"key"},
        "secrets":{"A":"é".repeat(2044)}
    }));
    assert_eq!(
        serde_jcs::to_vec(&exact_secret_document.secrets)
            .unwrap()
            .len(),
        brain_protocol::MAX_SESSION_SECRET_DOCUMENT_BYTES
    );
    validate_create_request(&exact_secret_document)
        .expect("an exact-size custody document is accepted");
    let oversized_secret_document = typed_create(json!({
        "model":{"provider":"anthropic","name":"model","api_key":"key"},
        "secrets":{"A":"é".repeat(2045)}
    }));
    assert!(matches!(
        validate_create_request(&oversized_secret_document),
        Err(BrainError::Invalid(_))
    ));
}

#[tokio::test]
async fn context_capacity_rejects_before_custody_or_environment_effects() {
    let data_dir = std::env::temp_dir().join(format!(
        "brain-context-admission-{}-{}",
        std::process::id(),
        crate::wall_ms()
    ));
    let custody = Arc::new(CountingCustody::default());
    let brain = Brain::with_parts_and_services(
        BrainConfig::default(),
        Journal::new_memory("brain-context-admission"),
        custody.clone(),
        Arc::new(DisabledToolExecutor),
        BrainServices::default(),
        crate::provider::fake::unscripted_factory(),
    );
    let error = brain
        .create_session(
            typed_create(json!({
                "model": {
                    "provider":"anthropic",
                    "name":"unknown-small-model",
                    "api_key":"key",
                    "max_output_tokens": brain_protocol::MAX_MODEL_OUTPUT_TOKENS
                }
            })),
            Some("context-admission"),
        )
        .await
        .expect_err("the conservative default window cannot fit a 128K output reserve");
    assert!(matches!(error, BrainError::Invalid(_)));
    assert_eq!(custody.encrypts.load(Ordering::Relaxed), 0);
    assert!(
        !data_dir.exists(),
        "Environment staging must not run after a pure validation failure"
    );
}

fn submit_policy() -> crate::config::ServerToolPolicy {
    crate::config::ServerToolPolicy {
        capability: "brain.submit".into(),
        scope: brain_protocol::session::ExternalToolScope::Root,
        completion: brain_protocol::session::ExternalToolCompletion::ReturnDirect,
        effect: brain_protocol::session::ExternalToolEffect::ReplaySafe,
        max_input_bytes: 1024,
    }
}

#[derive(Default)]
struct RecoveryExecutor {
    calls: AtomicUsize,
    call_ids: Mutex<Vec<String>>,
}

#[derive(Default)]
struct CancellationResistantExecutor {
    calls: AtomicUsize,
    entered: Notify,
    released: AtomicBool,
    release_waiters: Notify,
}

impl CancellationResistantExecutor {
    fn release(&self) {
        self.released.store(true, Ordering::Release);
        self.release_waiters.notify_waiters();
    }
}

#[async_trait::async_trait]
impl ToolExecutor for CancellationResistantExecutor {
    fn supports(&self, capability: &str) -> bool {
        capability == "brain.submit"
    }

    async fn call(
        &self,
        capability: &str,
        request: ExternalToolCallRequest,
        _cancel: CancellationToken,
    ) -> Result<ExternalToolCallResponse> {
        assert_eq!(capability, "brain.submit");
        self.calls.fetch_add(1, Ordering::AcqRel);
        self.entered.notify_waiters();
        loop {
            let notified = self.release_waiters.notified();
            if self.released.load(Ordering::Acquire) {
                break;
            }
            notified.await;
        }
        Ok(serde_json::from_value(json!({
            "outcome": "completed",
            "content": "accepted after the END fence",
            "is_error": false,
            "disposition": "complete_turn",
            "result": request.input,
            "result_metadata": {"recovered": "true"}
        }))
        .expect("valid resistant-effect response"))
    }
}

struct ReservationStorage {
    journal: Journal,
    prepares: AtomicUsize,
    aborts: AtomicUsize,
    fail_next_abort: AtomicBool,
    fail_next_write_before_effect: AtomicBool,
    saw_durable_reservation: AtomicBool,
    writes: AtomicUsize,
    pending: Mutex<HashMap<String, crate::storage::StorageUploadRequest>>,
    staged: Mutex<HashSet<String>>,
    objects: Mutex<HashMap<String, crate::storage::StorageObject>>,
}

struct DirectTransferPreparation;

#[async_trait::async_trait]
impl crate::environment::EnvironmentPort for DirectTransferPreparation {
    async fn resolve_binding(
        &self,
        _binding: brain_protocol::environment::SealedBinding,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::ResolvedBinding> {
        panic!("direct transfer test does not resolve tool bindings")
    }

    async fn submit(
        &self,
        _request: brain_protocol::environment::SubmitRequest,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::SubmitReceipt> {
        panic!("direct transfer test does not submit tools")
    }

    async fn observe(
        &self,
        _request: brain_protocol::environment::ObserveRequest,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::OperationObservation>
    {
        panic!("direct transfer test does not observe tools")
    }

    async fn cancel(
        &self,
        _request: brain_protocol::environment::CancelRequest,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::CancellationReceipt>
    {
        panic!("direct transfer test does not cancel tools")
    }

    async fn acknowledge_terminal(
        &self,
        _request: brain_protocol::environment::AcknowledgeTerminalRequest,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::Acknowledgement> {
        panic!("direct transfer test does not acknowledge tools")
    }
}

struct DirectTransferFiles {
    storage: Arc<ReservationStorage>,
    imports: AtomicUsize,
    exports: AtomicUsize,
}

#[derive(Default)]
struct UnknownManagedPorts {
    submits: AtomicUsize,
    status_calls: AtomicUsize,
    /// When set, `status` answers the retryable materialization-in-progress error the
    /// live plane returns while the detached submit still drives the launch.
    status_materializing: AtomicBool,
    dematerialize_calls: AtomicUsize,
    fail_next_dematerialize: AtomicBool,
    block_submit: AtomicBool,
    submit_started: tokio::sync::Notify,
    release_submit: tokio::sync::Notify,
}

struct TestBundleStorage;

#[async_trait::async_trait]
impl crate::storage::BundleStoragePort for TestBundleStorage {
    async fn store_bundle(
        &self,
        _root_id: &str,
        _bundle_digest: &str,
        _media_type: &str,
        _bytes: &[u8],
    ) -> Result<brain_protocol::environment::ObjectReference> {
        panic!("test bundle storage does not accept writes")
    }

    async fn prepare_bundle_fetch(
        &self,
        _root_id: &str,
        bundle_digest: &str,
    ) -> Result<brain_protocol::environment::BundleFetch> {
        Ok(serde_json::from_value(json!({
            "bundle_digest":bundle_digest,
            "url":"file:///test/tool-runtime.mjs",
            "headers":{},
            "expires_at_ms":crate::wall_ms() + 60_000,
            "max_bytes":1
        }))?)
    }

    async fn purge_root_bundles(&self, _root_id: &str) -> Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct CountingCustody {
    encrypts: AtomicUsize,
    decrypts: AtomicUsize,
}

#[async_trait::async_trait]
impl KeyCustody for CountingCustody {
    async fn encrypt(&self, session_id: &str, key: &ProviderKey) -> Result<Vec<u8>> {
        self.encrypts.fetch_add(1, Ordering::Relaxed);
        crate::keys::PlainCustody.encrypt(session_id, key).await
    }

    async fn decrypt(&self, session_id: &str, blob: &[u8]) -> Result<ProviderKey> {
        self.decrypts.fetch_add(1, Ordering::Relaxed);
        crate::keys::PlainCustody.decrypt(session_id, blob).await
    }
}

async fn connect_customer_process(
    coordinator: &Arc<crate::customer::CustomerCoordinator>,
    process_id: &str,
) -> (
    crate::customer::CustomerGrant,
    tokio::sync::mpsc::Receiver<crate::customer::CustomerCommand>,
    u64,
) {
    let grant = coordinator.grant("local", "app").await.unwrap();
    let proof = crate::customer::frame_proof(&grant.protocol);
    let connection_id = crate::mint_id("conn", 20);
    crate::customer::CustomerEnvironmentIngressPort::receive(
        coordinator.as_ref(),
        crate::customer::CustomerGatewayInput {
            route: crate::customer::CustomerGatewayRoute::Connect,
            connection_id: connection_id.clone(),
            request_id: crate::mint_id("req", 16),
            route_key: "$connect".into(),
            source_ip: "127.0.0.1".into(),
            subprotocol: Some(grant.protocol.clone()),
            body: None,
        },
    )
    .await
    .unwrap();
    let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
    coordinator
        .bind_local_sender(&connection_id, sender)
        .await
        .unwrap();
    crate::customer::CustomerEnvironmentIngressPort::receive(
        coordinator.as_ref(),
        crate::customer::CustomerGatewayInput {
            route: crate::customer::CustomerGatewayRoute::Message,
            connection_id: connection_id.clone(),
            request_id: crate::mint_id("req", 16),
            route_key: "$default".into(),
            source_ip: "127.0.0.1".into(),
            subprotocol: None,
            body: Some(
                json!({
                    "type":"register", "client_id":"app",
                    "process_id":process_id, "proof":proof
                })
                .to_string(),
            ),
        },
    )
    .await
    .unwrap();
    let Some(crate::customer::CustomerCommand::Ready { epoch }) = receiver.recv().await else {
        panic!("customer ready")
    };
    crate::customer::CustomerEnvironmentIngressPort::receive(
        coordinator.as_ref(),
        crate::customer::CustomerGatewayInput {
            route: crate::customer::CustomerGatewayRoute::Message,
            connection_id,
            request_id: crate::mint_id("req", 16),
            route_key: "$default".into(),
            source_ip: "127.0.0.1".into(),
            subprotocol: None,
            body: Some(
                json!({
                    "type":"register_tools", "epoch":epoch, "batch_id":"batch:test",
                    "proof":proof,
                    "registrations":[{
                        "registration":"lookup", "name":"lookup",
                        "contract_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    }]
                })
                .to_string(),
            ),
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        receiver.recv().await,
        Some(crate::customer::CustomerCommand::Registered { .. })
    ));
    (grant, receiver, epoch)
}

impl ReservationStorage {
    fn new(journal: Journal) -> Self {
        Self {
            journal,
            prepares: AtomicUsize::new(0),
            aborts: AtomicUsize::new(0),
            fail_next_abort: AtomicBool::new(false),
            fail_next_write_before_effect: AtomicBool::new(false),
            saw_durable_reservation: AtomicBool::new(false),
            writes: AtomicUsize::new(0),
            pending: Mutex::new(HashMap::new()),
            staged: Mutex::new(HashSet::new()),
            objects: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl crate::environment::SessionPreparationPort for DirectTransferPreparation {
    async fn prepare(
        &self,
        _request: brain_protocol::environment::PrepareSessionRequest,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::PreparedSession> {
        Ok(
            serde_json::from_value(json!({"preparation_ref":"prep_direct_transfer"}))
                .expect("prepared session"),
        )
    }

    async fn materialize(
        &self,
        request: brain_protocol::environment::CreateSandboxRequest,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::SandboxStatus> {
        Ok(serde_json::from_value(json!({
            "state":"running",
            "target":request.target,
            "generation":request.generation_intent,
            "target_ref":"target_direct_transfer",
            "changed_at_ms":crate::wall_ms(),
            "expires_at_ms":crate::wall_ms() + 60 * 60 * 1_000,
        }))
        .expect("running environment"))
    }

    async fn dematerialize(
        &self,
        target: brain_protocol::environment::SandboxTarget,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::SandboxStatus> {
        Ok(serde_json::from_value(json!({
            "state":"terminated",
            "target":target,
            "changed_at_ms":crate::wall_ms(),
            "expires_at_ms":null,
        }))
        .expect("terminated environment"))
    }

    async fn purge_tree(&self, _root_id: &str) -> crate::environment::EnvironmentResult<()> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::environment::EnvironmentPort for UnknownManagedPorts {
    async fn resolve_binding(
        &self,
        binding: brain_protocol::environment::SealedBinding,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::ResolvedBinding> {
        Ok(serde_json::from_value(json!({
            "binding_ref":binding.binding_id,
            "capabilities":["execution","session_preparation"],
            "environment_id":"environment_managed_test",
            "limits":{
                "max_inline_input_bytes":brain_protocol::MAX_MANAGED_TOOL_INPUT_BYTES,
                "max_inline_result_bytes":brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
                "max_wait_ms":1
            },
            "recovery":"retained"
        }))
        .expect("valid test managed binding"))
    }

    async fn submit(
        &self,
        _request: brain_protocol::environment::SubmitRequest,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::SubmitReceipt> {
        self.submits.fetch_add(1, Ordering::AcqRel);
        if self.block_submit.load(Ordering::Acquire) {
            self.submit_started.notify_one();
            self.release_submit.notified().await;
        }
        Err(serde_json::from_value(json!({
            "code":"operation_unknown",
            "message":"guest Submit may have run before the physical generation was lost",
            "retryable":false
        }))
        .expect("operation-unknown Environment error"))
    }

    async fn observe(
        &self,
        _request: brain_protocol::environment::ObserveRequest,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::OperationObservation>
    {
        panic!("an unknown submit has no operation receipt to observe")
    }

    async fn cancel(
        &self,
        _request: brain_protocol::environment::CancelRequest,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::CancellationReceipt>
    {
        panic!("an unknown submit has no operation receipt to cancel")
    }

    async fn acknowledge_terminal(
        &self,
        _request: brain_protocol::environment::AcknowledgeTerminalRequest,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::Acknowledgement> {
        panic!("an unknown submit has no terminal receipt to acknowledge")
    }
}

#[async_trait::async_trait]
impl crate::environment::SessionPreparationPort for UnknownManagedPorts {
    async fn prepare(
        &self,
        _request: brain_protocol::environment::PrepareSessionRequest,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::PreparedSession> {
        Ok(
            serde_json::from_value(json!({"preparation_ref":"prep_managed_test"}))
                .expect("valid test preparation"),
        )
    }

    async fn materialize(
        &self,
        _request: brain_protocol::environment::CreateSandboxRequest,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::SandboxStatus> {
        panic!("OperationUnknown must not authorize replacement materialization")
    }

    async fn dematerialize(
        &self,
        target: brain_protocol::environment::SandboxTarget,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::SandboxStatus> {
        self.dematerialize_calls.fetch_add(1, Ordering::AcqRel);
        if self.fail_next_dematerialize.swap(false, Ordering::AcqRel) {
            return Err(serde_json::from_value(json!({
                "code":"temporarily_unavailable",
                "message":"injected crash boundary before terminal sandbox cleanup",
                "retryable":true
            }))
            .expect("transient Environment error"));
        }
        Ok(serde_json::from_value(json!({
            "state":"terminated",
            "target":target,
            "generation":"gen_unknown_submit",
            "target_ref":"tgt_unknown_submit",
            "changed_at_ms":crate::wall_ms(),
            "expires_at_ms":null,
            "reason":"operation_unknown_reconciled"
        }))
        .expect("terminal unknown target status"))
    }

    async fn purge_tree(&self, _root_id: &str) -> crate::environment::EnvironmentResult<()> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::environment::SandboxFilesPort for UnknownManagedPorts {
    async fn status(
        &self,
        target: brain_protocol::environment::SandboxTarget,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::SandboxStatus> {
        self.status_calls.fetch_add(1, Ordering::AcqRel);
        if self.status_materializing.load(Ordering::Acquire) {
            return Err(serde_json::from_value(json!({
                "code": "temporarily_unavailable",
                "retryable": true,
                "message": "sandbox materialization is in progress",
                "details": {}
            }))
            .expect("retryable status error"));
        }
        Ok(serde_json::from_value(json!({
            "state":"running",
            "target":target,
            "generation":"gen_unknown_submit",
            "target_ref":"tgt_unknown_submit",
            "changed_at_ms":crate::wall_ms(),
            "expires_at_ms":crate::wall_ms() + 60_000
        }))
        .expect("fenced unknown target status"))
    }

    async fn list(
        &self,
        _request: crate::environment::SandboxFileListRequest,
    ) -> crate::environment::EnvironmentResult<crate::environment::SandboxFileList> {
        unreachable!("unused")
    }

    async fn stat(
        &self,
        _request: brain_protocol::environment::SandboxFileRequest,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::FileEntry> {
        unreachable!("unused")
    }

    async fn read(
        &self,
        _request: brain_protocol::environment::SandboxFileRequest,
    ) -> crate::environment::EnvironmentResult<crate::environment::SandboxFileContent> {
        unreachable!("unused")
    }

    async fn write(
        &self,
        _request: brain_protocol::environment::SandboxFileWriteRequest,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::SandboxFileWriteResult>
    {
        unreachable!("unused")
    }

    async fn find(
        &self,
        _request: crate::environment::SandboxSearchRequest,
    ) -> crate::environment::EnvironmentResult<crate::environment::SandboxFileList> {
        unreachable!("unused")
    }

    async fn grep(
        &self,
        _request: crate::environment::SandboxSearchRequest,
    ) -> crate::environment::EnvironmentResult<crate::environment::SandboxFileList> {
        unreachable!("unused")
    }

    async fn transfer(
        &self,
        _request: brain_protocol::environment::SandboxCopyRequest,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::SandboxCopyResult> {
        unreachable!("unused")
    }
}

fn direct_transfer_file(
    path: &str,
    bytes: u64,
    sha256: Option<&str>,
) -> brain_protocol::environment::FileEntry {
    serde_json::from_value(json!({
        "path":path,
        "kind":"file",
        "bytes":bytes,
        "sha256":sha256,
        "modified_at_ms":crate::wall_ms(),
    }))
    .expect("direct transfer file")
}

#[async_trait::async_trait]
impl crate::environment::SandboxFilesPort for DirectTransferFiles {
    async fn status(
        &self,
        _target: brain_protocol::environment::SandboxTarget,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::SandboxStatus> {
        unreachable!("status is not used by the direct transfer test")
    }

    async fn list(
        &self,
        _request: crate::environment::SandboxFileListRequest,
    ) -> crate::environment::EnvironmentResult<crate::environment::SandboxFileList> {
        unreachable!("list is not used by the direct transfer test")
    }

    async fn stat(
        &self,
        request: brain_protocol::environment::SandboxFileRequest,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::FileEntry> {
        Ok(direct_transfer_file(
            &String::from(request.path),
            2 * 1024 * 1024,
            None,
        ))
    }

    async fn read(
        &self,
        _request: brain_protocol::environment::SandboxFileRequest,
    ) -> crate::environment::EnvironmentResult<crate::environment::SandboxFileContent> {
        unreachable!("read is not used by the direct transfer test")
    }

    async fn write(
        &self,
        _request: brain_protocol::environment::SandboxFileWriteRequest,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::SandboxFileWriteResult>
    {
        unreachable!("write is not used by the direct transfer test")
    }

    async fn find(
        &self,
        _request: crate::environment::SandboxSearchRequest,
    ) -> crate::environment::EnvironmentResult<crate::environment::SandboxFileList> {
        unreachable!("find is not used by the direct transfer test")
    }

    async fn grep(
        &self,
        _request: crate::environment::SandboxSearchRequest,
    ) -> crate::environment::EnvironmentResult<crate::environment::SandboxFileList> {
        unreachable!("grep is not used by the direct transfer test")
    }

    async fn transfer(
        &self,
        request: brain_protocol::environment::SandboxCopyRequest,
    ) -> crate::environment::EnvironmentResult<brain_protocol::environment::SandboxCopyResult> {
        let path = String::from(request.path.clone());
        let result = match request.direction {
            brain_protocol::environment::SandboxCopyRequestDirection::Export => {
                self.exports.fetch_add(1, Ordering::Relaxed);
                let transfer_id = String::from(request.transfer.transfer_id.clone());
                self.storage
                    .staged
                    .lock()
                    .expect("staged uploads")
                    .insert(transfer_id);
                json!({
                    "operation_id":request.operation_id,
                    "request_digest":request.request_digest,
                    "replayed":false,
                    "file":direct_transfer_file(&path, 2 * 1024 * 1024, None),
                    "object":{
                        "object_id":request.transfer.object_id,
                        "bytes":2 * 1024 * 1024,
                        "sha256":"d".repeat(64),
                    }
                })
            }
            brain_protocol::environment::SandboxCopyRequestDirection::Import => {
                self.imports.fetch_add(1, Ordering::Relaxed);
                let object = request.object.expect("import object");
                json!({
                    "operation_id":request.operation_id,
                    "request_digest":request.request_digest,
                    "replayed":false,
                    "file":direct_transfer_file(&path, object.bytes, Some(&object.sha256)),
                    "object":null,
                })
            }
        };
        Ok(serde_json::from_value(result).expect("sandbox copy result"))
    }
}

#[async_trait::async_trait]
impl crate::storage::SessionStoragePort for ReservationStorage {
    async fn list(
        &self,
        _session_id: &str,
        _prefix: Option<&str>,
        _cursor: Option<&str>,
        _limit: u32,
    ) -> Result<crate::storage::StoragePage> {
        Ok(crate::storage::StoragePage {
            objects: Vec::new(),
            next_cursor: None,
        })
    }

    async fn stat(&self, session_id: &str, key: &str) -> Result<crate::storage::StorageObject> {
        self.objects
            .lock()
            .expect("storage objects")
            .get(&format!("{session_id}\0{key}"))
            .cloned()
            .ok_or_else(|| BrainError::FileNotFound(key.into()))
    }

    async fn read(&self, _session_id: &str, key: &str, _max_bytes: u64) -> Result<Vec<u8>> {
        Err(BrainError::FileNotFound(key.into()))
    }

    async fn write(
        &self,
        request: crate::storage::StorageWriteRequest,
    ) -> Result<crate::storage::StorageObject> {
        self.writes.fetch_add(1, Ordering::Relaxed);
        if self
            .fail_next_write_before_effect
            .swap(false, Ordering::Relaxed)
        {
            return Err(BrainError::Journal(
                "simulated crash before inline publication".into(),
            ));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&request.content_base64)
            .map_err(|_| BrainError::Invalid("test content is not base64".into()))?;
        let now = crate::wall_ms();
        let map_key = format!("{}\0{}", request.session_id, request.key);
        let created_at_ms = self
            .objects
            .lock()
            .expect("storage objects")
            .get(&map_key)
            .map_or(now, |object| object.created_at_ms);
        let object = crate::storage::StorageObject {
            key: request.key,
            bytes: bytes.len() as u64,
            sha256: hex::encode(Sha256::digest(&bytes)),
            content_type: request.content_type,
            publication_id: Some(request.publication_id),
            created_at_ms,
            updated_at_ms: now,
        };
        self.objects
            .lock()
            .expect("storage objects")
            .insert(map_key, object.clone());
        Ok(object)
    }

    async fn prepare_download(
        &self,
        session_id: &str,
        key: &str,
    ) -> Result<crate::storage::StorageTransferTicket> {
        let object = self.stat(session_id, key).await?;
        Ok(crate::storage::StorageTransferTicket {
            object_id: crate::storage::stored_object_id(&object.sha256),
            transfer_id: crate::mint_id("xfer", 24),
            method: "GET".into(),
            url: "https://storage.invalid/download".into(),
            headers: HashMap::new(),
            expires_at_ms: crate::wall_ms() + 60 * 60 * 1_000,
            max_bytes: object.bytes,
        })
    }

    async fn prepare_upload(
        &self,
        request: crate::storage::StorageUploadRequest,
    ) -> Result<crate::storage::StorageTransferTicket> {
        self.prepares.fetch_add(1, Ordering::Relaxed);
        let head = self.journal.get_head(&request.session_id).await?;
        let durable = head.doc.storage_reserved_bytes == request.bytes
            && head.doc.storage_upload.as_ref().is_some_and(|upload| {
                upload.transfer_id == request.transfer_id
                    && upload.state == UploadReservationState::Reserved
            });
        self.saw_durable_reservation
            .store(durable, Ordering::Relaxed);
        self.pending
            .lock()
            .expect("pending uploads")
            .insert(request.transfer_id.clone(), request.clone());
        Ok(crate::storage::StorageTransferTicket {
            object_id: crate::storage::pending_object_id(&request.transfer_id),
            transfer_id: request.transfer_id,
            method: "PUT".into(),
            url: "https://storage.invalid/upload".into(),
            headers: HashMap::new(),
            expires_at_ms: request.expires_at_ms,
            max_bytes: request.bytes,
        })
    }

    async fn complete_upload(
        &self,
        session_id: &str,
        transfer_id: &str,
    ) -> Result<crate::storage::StorageObject> {
        if !self
            .staged
            .lock()
            .expect("staged uploads")
            .contains(transfer_id)
        {
            return Err(BrainError::FileNotFound(transfer_id.into()));
        }
        let request = self
            .pending
            .lock()
            .expect("pending uploads")
            .get(transfer_id)
            .cloned()
            .ok_or_else(|| BrainError::FileNotFound(transfer_id.into()))?;
        if request.session_id != session_id {
            return Err(BrainError::FileNotFound(transfer_id.into()));
        }
        let now = crate::wall_ms();
        let object = crate::storage::StorageObject {
            key: request.key.clone(),
            bytes: request.bytes,
            sha256: request.sha256.unwrap_or_else(|| "d".repeat(64)),
            content_type: request.content_type,
            publication_id: Some(request.transfer_id),
            created_at_ms: now,
            updated_at_ms: now,
        };
        self.objects
            .lock()
            .expect("storage objects")
            .insert(format!("{session_id}\0{}", request.key), object.clone());
        Ok(object)
    }

    async fn abort_upload(&self, _session_id: &str, transfer_id: &str) -> Result<()> {
        self.aborts.fetch_add(1, Ordering::Relaxed);
        if self.fail_next_abort.swap(false, Ordering::Relaxed) {
            return Err(BrainError::Journal("transient staging deletion".into()));
        }
        self.pending
            .lock()
            .expect("pending uploads")
            .remove(transfer_id);
        self.staged
            .lock()
            .expect("staged uploads")
            .remove(transfer_id);
        Ok(())
    }

    async fn delete(&self, session_id: &str, key: &str) -> Result<()> {
        self.objects
            .lock()
            .expect("storage objects")
            .remove(&format!("{session_id}\0{key}"));
        Ok(())
    }

    async fn purge_session_page(
        &self,
        _session_id: &str,
        _cursor: Option<&str>,
    ) -> Result<crate::storage::StoragePurgePage> {
        Ok(crate::storage::StoragePurgePage {
            deleted_versions: 0,
            deleted_markers: 0,
            next_cursor: None,
        })
    }
}

#[test]
fn direct_sandbox_transfer_admission_is_count_and_byte_bounded() {
    let brain = Brain::with_parts_and_services(
        BrainConfig::default(),
        Journal::new_memory("brain-direct-transfer-admission"),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices::default(),
        crate::provider::fake::unscripted_factory(),
    );
    let entry = |session_id: &str, id: &str, bytes: u64| DirectSandboxTransfer {
        session_id: session_id.into(),
        storage_key: direct_sandbox_transfer_key(id),
        declared_bytes: bytes,
        expires_at_ms: crate::wall_ms() + 60_000,
        cleanup_at_ms: crate::wall_ms() + 120_000,
        storage_transfer_id: None,
        destination: None,
        state: DirectSandboxTransferState::Preparing,
    };
    for index in 0..MAX_PENDING_SANDBOX_TRANSFERS_PER_SESSION {
        let id = format!("sbxfer_count_{index}");
        brain
            .reserve_direct_sandbox_transfer(&id, entry("ses_count", &id, 1))
            .expect("exact per-session count is admitted");
    }
    let over = "sbxfer_count_over";
    assert!(matches!(
        brain.reserve_direct_sandbox_transfer(over, entry("ses_count", over, 1)),
        Err(BrainError::Overloaded)
    ));

    let bytes_brain = Brain::with_parts_and_services(
        BrainConfig::default(),
        Journal::new_memory("brain-direct-transfer-bytes"),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices::default(),
        crate::provider::fake::unscripted_factory(),
    );
    bytes_brain
        .reserve_direct_sandbox_transfer(
            "sbxfer_bytes_exact",
            entry(
                "ses_bytes",
                "sbxfer_bytes_exact",
                MAX_PENDING_SANDBOX_TRANSFER_BYTES_PER_SESSION,
            ),
        )
        .expect("exact per-session bytes are admitted");
    assert!(matches!(
        bytes_brain.reserve_direct_sandbox_transfer(
            "sbxfer_bytes_over",
            entry("ses_bytes", "sbxfer_bytes_over", 1),
        ),
        Err(BrainError::Overloaded)
    ));
}

#[tokio::test]
async fn direct_sandbox_transfers_stage_hidden_bytes_and_replay_only_exact_success() {
    let journal = Journal::new_memory("brain-direct-sandbox-transfers");
    let storage = Arc::new(ReservationStorage::new(journal.clone()));
    let files = Arc::new(DirectTransferFiles {
        storage: storage.clone(),
        imports: AtomicUsize::new(0),
        exports: AtomicUsize::new(0),
    });
    let preparation = Arc::new(DirectTransferPreparation);
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            storage_transfer_ttl: Duration::from_secs(60 * 60),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(crate::adapter::DisabledToolExecutor),
        BrainServices {
            session_storage: Some(storage.clone()),
            environments: test_environment_registry(
                "test.transfer",
                preparation.clone(),
                preparation,
                Some(files.clone()),
            ),
            ..BrainServices::default()
        },
        crate::provider::fake::unscripted_factory(),
    );
    let session = brain
        .create_session(
            typed_create(json!({
                "model":{"provider":"anthropic", "name":"direct-transfer", "api_key":"key"},
                "environments":{"workspace":{"extension":"test.transfer","protocol":"environment/v1","profile":{"kind":"computer","platform":"linux-amd64","network":"none","recovery":"retained"},"configuration":{}}}
            })),
            Some("direct-sandbox-transfer"),
        )
        .await
        .expect("create direct-transfer session");
    let session_id = session.id.to_string();
    let status = brain
        .materialize_environment(&session_id, "workspace")
        .await
        .expect("materialize environment");
    let generation = status
        .generation
        .map(String::from)
        .expect("materialized generation");

    let download = brain
        .sandbox_file_prepare_download(
            &session_id,
            "workspace",
            generation.clone(),
            "/workspace/source.bin".into(),
        )
        .await
        .expect("prepare sandbox download");
    assert!(download.transfer_id.starts_with("sbxfer_"));
    assert_eq!(download.method, "GET");
    assert_eq!(files.exports.load(Ordering::Relaxed), 1);
    let download_key = brain
        .direct_sandbox_transfers
        .lock()
        .expect("direct transfers")
        .get(&download.transfer_id)
        .expect("retained download")
        .storage_key
        .clone();
    assert!(crate::storage::is_internal_storage_key(&download_key));
    assert!(matches!(
        brain.storage_stat(&session_id, &download_key).await,
        Err(BrainError::Invalid(_))
    ));

    let upload = brain
        .sandbox_file_prepare_upload(
            &session_id,
            "workspace",
            generation,
            "/workspace/upload.bin".into(),
            2 * 1024 * 1024,
            "e".repeat(64),
            true,
        )
        .await
        .expect("prepare sandbox upload");
    let (underlying, upload_key) = {
        let transfers = brain
            .direct_sandbox_transfers
            .lock()
            .expect("direct transfers");
        let transfer = transfers.get(&upload.transfer_id).expect("retained upload");
        (
            transfer
                .storage_transfer_id
                .clone()
                .expect("underlying storage transfer"),
            transfer.storage_key.clone(),
        )
    };
    assert_ne!(upload.transfer_id, underlying);
    storage
        .staged
        .lock()
        .expect("staged uploads")
        .insert(underlying);
    let completed = brain
        .sandbox_file_complete_upload(&session_id, &upload.transfer_id)
        .await
        .expect("complete sandbox upload");
    assert_eq!(
        String::from(completed.path.clone()),
        "/workspace/upload.bin"
    );
    let replayed = brain
        .sandbox_file_complete_upload(&session_id, &upload.transfer_id)
        .await
        .expect("replay exact completed outcome");
    assert_eq!(String::from(replayed.path), "/workspace/upload.bin");
    assert_eq!(files.imports.load(Ordering::Relaxed), 1);
    assert!(
        storage
            .objects
            .lock()
            .expect("storage objects")
            .get(&format!("{session_id}\0{upload_key}"))
            .is_none(),
        "successful import best-effort purges hidden staging"
    );

    let restarted = Brain::with_parts_and_services(
        BrainConfig::default(),
        journal,
        Arc::new(crate::keys::PlainCustody),
        Arc::new(crate::adapter::DisabledToolExecutor),
        BrainServices {
            session_storage: Some(storage),
            ..BrainServices::default()
        },
        crate::provider::fake::unscripted_factory(),
    );
    assert!(matches!(
        restarted
            .sandbox_file_complete_upload(&session_id, &upload.transfer_id)
            .await,
        Err(BrainError::SandboxTransferUnknown(_))
    ));
}

#[async_trait::async_trait]
impl ToolExecutor for RecoveryExecutor {
    fn supports(&self, capability: &str) -> bool {
        capability == "brain.submit"
    }

    async fn call(
        &self,
        capability: &str,
        request: ExternalToolCallRequest,
        _cancel: CancellationToken,
    ) -> Result<ExternalToolCallResponse> {
        assert_eq!(capability, "brain.submit");
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.call_ids
            .lock()
            .expect("recovery call ids")
            .push(request.call_id.to_string());
        Ok(serde_json::from_value(json!({
            "outcome": "completed",
            "content": "accepted after recovery",
            "is_error": false,
            "disposition": "complete_turn",
            "result": request.input,
            "result_metadata": {"recovered": "true"}
        }))
        .expect("valid recovery response"))
    }
}

#[test]
fn child_fork_excludes_spawning_tool_use_and_partial_sibling_results() {
    let prompt = Message::user_text("delegate the focused task");
    let spawning = Message::assistant(vec![
        ContentBlock::ToolUse {
            id: "op_spawn".into(),
            name: "subagents".into(),
            input: json!({"action":"spawn","prompt":"work"}),
        },
        ContentBlock::ToolUse {
            id: "op_sibling".into(),
            name: "subagents".into(),
            input: json!({"action":"spawn","prompt":"other"}),
        },
    ]);
    let partial = Message::tool_results(vec![ContentBlock::ToolResult {
        tool_use_id: "op_sibling".into(),
        content: "created sibling".into(),
        is_error: false,
    }]);
    let history = vec![prompt.clone(), spawning.clone(), partial];
    assert_eq!(
        complete_fork_projection(&history),
        std::slice::from_ref(&prompt)
    );

    let complete = Message::tool_results(vec![
        ContentBlock::ToolResult {
            tool_use_id: "op_spawn".into(),
            content: "created".into(),
            is_error: false,
        },
        ContentBlock::ToolResult {
            tool_use_id: "op_sibling".into(),
            content: "created sibling".into(),
            is_error: false,
        },
    ]);
    let closed = vec![prompt, spawning, complete];
    assert_eq!(complete_fork_projection(&closed), closed.as_slice());
}

#[test]
fn child_fork_modes_select_only_complete_recent_turns() {
    let history = vec![
        Message::user_text("one"),
        Message::assistant(vec![ContentBlock::text("answer one")]),
        Message::user_text("two"),
        Message::assistant(vec![ContentBlock::text("answer two")]),
        Message::user_text("three"),
    ];
    let (all, all_turns) = select_fork_history(&history, &ForkTurns::All);
    assert_eq!(all, history);
    assert_eq!(all_turns, 3);
    let (last_two, turns) = select_fork_history(&history, &ForkTurns::Last(2));
    assert_eq!(turns, 2);
    assert_eq!(last_two.first(), Some(&Message::user_text("two")));
    assert_eq!(ForkTurns::parse(None).unwrap(), ForkTurns::All);
    assert!(ForkTurns::parse(Some("0")).is_err());
}

#[tokio::test]
async fn descendants_share_one_root_scoped_custody_decryption_cell() {
    let custody = Arc::new(CountingCustody::default());
    let brain = Brain::with_parts_and_services(
        BrainConfig::default(),
        Journal::new_memory("brain-root-secret-cache"),
        custody.clone(),
        Arc::new(DisabledToolExecutor),
        BrainServices::default(),
        crate::provider::fake::unscripted_factory(),
    );
    let created = brain
        .create_session(
            typed_create(json!({
                "model": {
                    "provider":"anthropic",
                    "name":"model",
                    "api_key":"root-provider-secret"
                }
            })),
            Some("root-secret-cache"),
        )
        .await
        .unwrap();
    assert_eq!(custody.encrypts.load(Ordering::Relaxed), 1);
    assert_eq!(custody.decrypts.load(Ordering::Relaxed), 0);

    let root = brain
        .journal
        .get_head(&created.id.to_string())
        .await
        .unwrap();
    let mut child = root.doc.clone();
    child.parent_id = Some(root.session_id.clone());
    child.ancestor_ids = vec![root.session_id.clone()];
    child.depth = 1;
    let (root_cell, root_secrets) = brain.root_execution_secrets(&root.doc).await.unwrap();
    let (child_cell, child_secrets) = brain.root_execution_secrets(&child).await.unwrap();
    assert!(Arc::ptr_eq(&root_cell, &child_cell));
    assert_eq!(root_secrets.key.expose(), "root-provider-secret");
    assert_eq!(child_secrets.key.expose(), "root-provider-secret");
    assert_eq!(custody.decrypts.load(Ordering::Relaxed), 1);

    drop(root_secrets);
    drop(child_secrets);
    drop(root_cell);
    drop(child_cell);
    let (_new_cell, _new_secrets) = brain.root_execution_secrets(&root.doc).await.unwrap();
    assert_eq!(
        custody.decrypts.load(Ordering::Relaxed),
        2,
        "the weak cache must release secret material after the last residency"
    );
}

#[tokio::test]
async fn child_create_atomically_admits_prompt_and_rebuilds_exact_parent_fork() {
    let data_dir = std::env::temp_dir().join(format!(
        "brain-child-fork-{}-{}",
        std::process::id(),
        crate::wall_ms()
    ));
    let journal = Journal::new_memory("brain-child-fork");
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script([
        Scripted::Text("parent answer".into()),
        Scripted::Text("child answer".into()),
    ]);
    let provider = fake.clone();
    let provider_factory: ProviderFactory = Arc::new(move |_| provider.clone());
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            idle_discard: Duration::from_secs(300),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices::default(),
        provider_factory,
    );
    let root = brain
        .create_session(
            typed_create(json!({
                "model": {
                    "provider":"anthropic", "name":"child-fork-test", "api_key":"key"
                }
            })),
            Some("child-fork-root"),
        )
        .await
        .unwrap();
    let root_id = root.id.to_string();
    brain
        .message(
            &root_id,
            MessageRequestContent::String("parent prompt".parse().unwrap()),
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if journal.get_head(&root_id).await.unwrap().doc.turn.is_none() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("parent turn completes");

    let child = brain
        .create_child(
            &root_id,
            "child prompt".into(),
            Some("focused".into()),
            None,
            Some("spawn-1"),
        )
        .await
        .unwrap();
    let child_id = child.id.to_string();
    assert_eq!(child.name.as_deref().map(String::as_str), Some("focused"));
    assert_eq!(
        child.context_fork.as_ref().map(|fork| fork.mode),
        Some(session::ContextForkMode::All)
    );
    let initial = journal.read_records_through(&child_id, 0, 1).await.unwrap();
    assert!(matches!(
        &initial[..],
        [Entry {
            record: Record::UserMessage {
                starts_turn: true,
                content,
                ..
            },
            ..
        }] if content == &vec![ContentBlock::text("child prompt")]
    ));
    let listed = journal
        .list_child_page(&crate::journal::ChildListQuery {
            parent_id: &root_id,
            limit: 10,
            cursor: None,
        })
        .await
        .unwrap();
    assert_eq!(listed.sessions.len(), 1);
    assert_eq!(listed.sessions[0].session_id, child_id);

    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if journal
                .get_head(&child_id)
                .await
                .unwrap()
                .doc
                .turn
                .is_none()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("child initial turn completes");
    let child_head = journal.get_head(&child_id).await.unwrap();
    let child_entries = journal.read_records(&child_id, 0).await.unwrap();
    let fork_context = materialize_context_fork(&brain, &child_head.doc)
        .await
        .unwrap();
    let history =
        materialize_session_history(&child_head.doc, &child_entries, &fork_context).unwrap();
    assert_eq!(history[0], Message::user_text("parent prompt"));
    assert_eq!(
        history[1],
        Message::assistant(vec![ContentBlock::text("parent answer")])
    );
    assert_eq!(history[2], Message::user_text("child prompt"));
    assert_eq!(
        history[3],
        Message::assistant(vec![ContentBlock::text("child answer")])
    );

    let replay = brain
        .create_child(
            &root_id,
            "child prompt".into(),
            Some("focused".into()),
            None,
            Some("spawn-1"),
        )
        .await
        .unwrap();
    assert_eq!(replay.id, child.id);
    assert!(matches!(
        brain
            .create_child(
                &root_id,
                "different prompt".into(),
                Some("focused".into()),
                None,
                Some("spawn-1"),
            )
            .await,
        Err(BrainError::IdempotencyConflict)
    ));
    fake.assert_drained(2, "ordinary child fork").unwrap();
    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
fn prefix_rebuild_is_deterministic() {
    let p = PrefixDoc {
        agentloop: None,
        system_prompt: Some("sp".into()),
        provider: "anthropic".into(),
        model_component: None,
        model: "claude-x".into(),
        base_url: Some("https://api.anthropic.com".into()),
        max_output_tokens: Some(2048),
        context_window_tokens: 32 * 1024,
        context_soft_tokens: 18 * 1024,
        context_hard_tokens: 22 * 1024,
        context_tail_tokens: 4 * 1024,
        context_summary_tokens: 4 * 1024,
        temperature: Some(0.5),
        reasoning_effort: None,
        provider_recovery_retries: 1,
        storage_max_object_bytes: crate::storage::DEFAULT_MAX_STORAGE_OBJECT_BYTES,
        storage_max_session_bytes: crate::storage::DEFAULT_MAX_SESSION_STORAGE_BYTES,
        storage_transfer_ttl_ms: crate::storage::DEFAULT_STORAGE_TRANSFER_TTL_MS,
        max_child_depth: 4,
        max_direct_children: 32,
        max_descendants: 256,
        network: serde_json::json!({"outbound":"none"}),
        customer_client_id: None,
        customer_submit_retries: 1,
        rendered_base: serde_json::Value::Null,
        rendered_base_digest: String::new(),
        prompt_cache_key: String::new(),
        tools: serde_json::from_value(json!([
            {
                "definition": {
                    "name":"run", "description":"run",
                    "contract_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "input_schema":{"type":"object"},
                    "output_schema":{"type":"object"}
                },
                "executor": {
                    "kind":"environment",
                    "environment":"workspace",
                    "artifact_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "requirements":{}
                }
            },
            {
                "definition": {
                    "name":"delegate", "description":"delegate",
                    "contract_digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "input_schema":{"type":"object"},
                    "output_schema":{"type":"string"}
                },
                "executor":{"kind":"engine", "capability":"brain.subagents"}
            }
        ])).unwrap(),
        environments: HashMap::new(),
        managed_bundles: vec![],
        official_capabilities: HashMap::new(),
        environment_enabled: true,
        shape: "1gb".into(),
        sync_interval_seconds: 600,
        environment_env_keys: vec![],
        metadata: HashMap::new(),
    };
    let (a, da) = build_prefix(&p, 512).unwrap();
    let (b, db) = build_prefix(&p, 512).unwrap();
    assert_eq!(a.digest(), b.digest());
    assert_eq!(da, db);
    assert_eq!(a.tools.len(), 2);
}

#[test]
fn pending_volatile_scan_routes_by_the_seal() {
    let task = |seq: u64, agent: &str, call: &str, detach: bool| Entry {
        seq,
        ts_ms: 0,
        record: Record::ToolCall {
            turn: "trn_test".into(),
            agent: agent.into(),
            call: call.into(),
            name: "delegate_under_any_name".into(),
            input: serde_json::json!({}),
            detach,
        },
    };
    let entries = vec![
        task(1, "root", "op_pending", false),
        task(2, "agt_child", "op_answered", false),
        Entry {
            seq: 3,
            ts_ms: 0,
            record: Record::ToolResult {
                turn: "trn_test".into(),
                agent: "agt_child".into(),
                call: "op_answered".into(),
                name: "delegate_under_any_name".into(),
                outcome: ToolOutcome::Completed,
                content: "done".into(),
                is_error: false,
                exit_code: None,
                duration_ms: 1,
                truncated: false,
            },
        },
        task(4, "root", "op_detached", true),
        Entry {
            seq: 5,
            ts_ms: 0,
            record: Record::CustomerCallIntent {
                turn: "trn_test".into(),
                call: "op_customer".into(),
                client_id: "app".into(),
                process_id: "process:test".into(),
                request_digest: "b".repeat(64),
                deadline_at_ms: 9_999_999,
            },
        },
        Entry {
            seq: 6,
            ts_ms: 0,
            record: Record::ToolCall {
                turn: "trn_test".into(),
                agent: "root".into(),
                call: "op_customer".into(),
                name: "customer_lookup".into(),
                input: serde_json::json!({"id":7}),
                detach: false,
            },
        },
    ];
    let prefix = PrefixDoc {
        model_component: None,
        agentloop: None,
        system_prompt: None,
        provider: "anthropic".into(),
        model: "m".into(),
        base_url: None,
        max_output_tokens: None,
        context_window_tokens: 32 * 1024,
        context_soft_tokens: 18 * 1024,
        context_hard_tokens: 22 * 1024,
        context_tail_tokens: 4 * 1024,
        context_summary_tokens: 4 * 1024,
        temperature: None,
        reasoning_effort: None,
        provider_recovery_retries: 1,
        storage_max_object_bytes: crate::storage::DEFAULT_MAX_STORAGE_OBJECT_BYTES,
        storage_max_session_bytes: crate::storage::DEFAULT_MAX_SESSION_STORAGE_BYTES,
        storage_transfer_ttl_ms: crate::storage::DEFAULT_STORAGE_TRANSFER_TTL_MS,
        max_child_depth: 4,
        max_direct_children: 32,
        max_descendants: 256,
        network: serde_json::json!({"outbound":"none"}),
        customer_client_id: Some("app".into()),
        customer_submit_retries: 1,
        rendered_base: serde_json::json!({}),
        rendered_base_digest: String::new(),
        prompt_cache_key: String::new(),
        tools: serde_json::from_value(json!([
            {
                "definition": {
                    "name":"delegate_under_any_name", "description":"delegate",
                    "contract_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "input_schema":{"type":"object"}, "output_schema":{"type":"string"}
                },
                "executor":{"kind":"engine", "capability":"brain.subagents"}
            },
            {
                "definition": {
                    "name":"customer_lookup", "description":"lookup",
                    "contract_digest":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                    "input_schema":{"type":"object"}, "output_schema":{"type":"object"}
                },
                "executor":{
                    "kind":"environment",
                    "environment":"app",
                    "callback_registration":"lookup",
                    "requirements":{}
                }
            }
        ]))
        .unwrap(),
        environments: HashMap::new(),
        managed_bundles: vec![],
        official_capabilities: HashMap::new(),
        environment_enabled: false,
        shape: "1gb".into(),
        sync_interval_seconds: 600,
        environment_env_keys: vec![],
        metadata: HashMap::new(),
    };
    let pending = pending_volatile(&entries, &prefix);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].call, "op_pending");
    let customer = pending_customer(&entries, &prefix, "tenant", "ses_test");
    assert_eq!(customer.len(), 1);
    assert_eq!(customer[0].call, "op_customer");
    assert_eq!(customer[0].intent.process_id, "process:test");
}

#[test]
fn pending_external_scan_recovers_only_unanswered_sealed_calls() {
    let prefix = PrefixDoc {
        model_component: None,
        agentloop: None,
        system_prompt: None,
        provider: "anthropic".into(),
        model: "m".into(),
        base_url: None,
        max_output_tokens: None,
        context_window_tokens: 32 * 1024,
        context_soft_tokens: 18 * 1024,
        context_hard_tokens: 22 * 1024,
        context_tail_tokens: 4 * 1024,
        context_summary_tokens: 4 * 1024,
        temperature: None,
        reasoning_effort: None,
        provider_recovery_retries: 1,
        storage_max_object_bytes: crate::storage::DEFAULT_MAX_STORAGE_OBJECT_BYTES,
        storage_max_session_bytes: crate::storage::DEFAULT_MAX_SESSION_STORAGE_BYTES,
        storage_transfer_ttl_ms: crate::storage::DEFAULT_STORAGE_TRANSFER_TTL_MS,
        max_child_depth: 4,
        max_direct_children: 32,
        max_descendants: 256,
        network: serde_json::json!({"outbound":"none"}),
        customer_client_id: None,
        customer_submit_retries: 1,
        rendered_base: serde_json::json!({}),
        rendered_base_digest: String::new(),
        prompt_cache_key: String::new(),
        tools: serde_json::from_value(json!([{
            "definition": {
                "name":"submit", "description":"submit",
                "contract_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "input_schema":{"type":"object"},
                "output_schema":{"type":"object"}
            },
            "executor": {"kind":"engine", "capability":"brain.submit"}
        }]))
        .unwrap(),
        environments: HashMap::new(),
        managed_bundles: vec![],
        official_capabilities: HashMap::from([("brain.submit".into(), submit_policy())]),
        environment_enabled: false,
        shape: "1gb".into(),
        sync_interval_seconds: 600,
        environment_env_keys: vec![],
        metadata: HashMap::new(),
    };
    let mut context = HashMap::new();
    context.insert("request".into(), "out_1".into());
    let mut entries = vec![
        Entry {
            seq: 1,
            ts_ms: 0,
            record: Record::UserMessage {
                turn: "trn_test".into(),
                content: vec![ContentBlock::text("answer")],
                starts_turn: false,
                metadata: context,
                idempotency_key_hash: None,
                request_hash: None,
            },
        },
        Entry {
            seq: 2,
            ts_ms: 0,
            record: Record::Assistant {
                turn: "trn_test".into(),
                agent: "root".into(),
                attempt_id: "att_aaaaaaaaaaaaaaaaaaaa".into(),
                content: vec![],
                stop: crate::message::StopReason::ToolUse,
            },
        },
        Entry {
            seq: 3,
            ts_ms: 0,
            record: Record::ToolCall {
                turn: "trn_test".into(),
                agent: "root".into(),
                call: "op_submit".into(),
                name: "submit".into(),
                input: json!({"answer": 42}),
                detach: false,
            },
        },
    ];
    let pending = pending_external(&entries, &prefix);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].call, "op_submit");
    assert_eq!(pending[0].context["request"], "out_1");
    assert!(!pending[0].parallel_batch);

    entries.push(Entry {
        seq: 4,
        ts_ms: 0,
        record: Record::ToolResult {
            turn: "trn_test".into(),
            agent: "root".into(),
            call: "op_submit".into(),
            name: "submit".into(),
            outcome: ToolOutcome::Completed,
            content: "done".into(),
            is_error: false,
            exit_code: None,
            duration_ms: 1,
            truncated: false,
        },
    });
    assert!(pending_external(&entries, &prefix).is_empty());
}

#[tokio::test]
async fn hydrate_replays_a_pending_replay_safe_external_call_with_the_same_id() {
    let data_dir = std::env::temp_dir().join(format!(
        "brain-external-recovery-{}-{}",
        std::process::id(),
        crate::wall_ms()
    ));
    std::fs::create_dir_all(&data_dir).expect("create recovery data dir");
    let journal = Journal::new_memory("brain-recovery-test");
    let executor = Arc::new(RecoveryExecutor::default());
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            idle_discard: Duration::from_secs(300),
            official_capabilities: HashMap::from([("brain.submit".into(), submit_policy())]),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        executor.clone(),
        BrainServices::default(),
        crate::provider::fake::unscripted_factory(),
    );
    let created = brain
        .create_session(
            typed_create_result(json!({
                "model": {
                    "provider": "anthropic",
                    "name": "unused-during-terminal-recovery",
                    "api_key": "sk-test"
                },
                "tools": {
                    "items": [{
                        "definition": {
                            "name": "submit",
                            "description": "Submit the final value",
                            "contract_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                            "input_schema": {"type": "object"},
                            "output_schema": {"type": "object"}
                        },
                        "executor": {"kind": "engine", "capability": "brain.submit"}
                    }]
                }
            }))
            .expect("valid create request"),
            None,
        )
        .await
        .expect("create session");
    let session_id = created.id.to_string();

    // Let the eager actor finish its initial environment-state decision before fencing its fold.
    for _ in 0..100 {
        if journal
            .get_head(&session_id)
            .await
            .expect("head while waiting")
            .last_seq
            >= 2
        {
            break;
        }
        tokio::task::yield_now().await;
    }

    let mut head = journal
        .claim(&session_id)
        .await
        .expect("claim for crash setup");
    let turn = "trn_aaaaaaaaaaaaaaaaaaaa".to_string();
    let call = "op_aaaaaaaaaaaaaaaa".to_string();
    let input = json!({"answer": 42});
    head.doc.state = SessionLifecycle::Open;
    head.doc.turn = Some(turn.clone());
    head.doc.turns += 1;
    head.doc.last_message_ms = Some(crate::wall_ms());
    let first_seq = head.last_seq + 1;
    let records = vec![
        (
            first_seq,
            Record::UserMessage {
                turn: turn.clone(),
                content: vec![ContentBlock::text("return a typed value")],
                starts_turn: false,
                metadata: HashMap::from([("request_id".into(), "out_test".into())]),
                idempotency_key_hash: None,
                request_hash: None,
            },
        ),
        (first_seq + 1, Record::TurnStarted { turn: turn.clone() }),
        (
            first_seq + 2,
            Record::Assistant {
                turn: turn.clone(),
                agent: "root".into(),
                attempt_id: "att_aaaaaaaaaaaaaaaaaaaa".into(),
                content: vec![ContentBlock::ToolUse {
                    id: call.clone(),
                    name: "submit".into(),
                    input: input.clone(),
                }],
                stop: crate::message::StopReason::ToolUse,
            },
        ),
        (
            first_seq + 3,
            Record::ToolCall {
                turn: turn.clone(),
                agent: "root".into(),
                call: call.clone(),
                name: "submit".into(),
                input: input.clone(),
                detach: false,
            },
        ),
    ];
    let mut lease = Lease {
        fence: head.fence,
        last_seq: head.last_seq,
        retention: head.retention,
    };
    journal
        .commit(&session_id, &mut lease, &records, &head.doc, first_seq + 3)
        .await
        .expect("commit pending external intent");
    let crash_entries = journal
        .read_records(&session_id, 0)
        .await
        .expect("read simulated crash records");
    let resolved = resolve_sealed_tools(&head.doc.prefix);
    assert!(
        resolved.iter().any(|tool| {
            tool.name == "submit"
                && matches!(
                    &tool.route,
                    crate::config::ToolRoute::Server(policy) if policy.capability == "brain.submit"
                )
        }),
        "resolved tools: {resolved:?}; prefix tools: {:?}",
        head.doc.prefix.tools
    );
    assert!(crash_entries.iter().any(|entry| matches!(
        &entry.record,
        Record::ToolCall { call: recorded, .. } if recorded == &call
    )));
    let pending = pending_external(&crash_entries, &head.doc.prefix);
    assert_eq!(
        pending.len(),
        1,
        "the committed server-tool intent is pending"
    );
    assert_eq!(pending[0].policy.capability, "brain.submit");
    // Model the durable failure transition that follows an observed owner loss. It releases
    // the stale lease and installs the bounded retry due-time atomically, so the background
    // scheduler can resume without customer traffic and without waiting another lease term.
    journal
        .defer_recovery(&session_id)
        .await
        .expect("release crashed owner and schedule recovery");

    let resident = hydrate(&brain, &session_id)
        .await
        .expect("hydrate and replay pending call");
    assert_eq!(executor.calls.load(Ordering::Relaxed), 1);
    assert_eq!(resident.st.head.state, SessionLifecycle::Open);
    assert!(resident.st.head.turn.is_none());
    assert_eq!(
        executor
            .call_ids
            .lock()
            .expect("recovery call ids")
            .as_slice(),
        [call.as_str()]
    );

    let recovered = journal
        .read_records(&session_id, first_seq + 3)
        .await
        .expect("recovery records");
    assert!(recovered.iter().any(|entry| matches!(
        &entry.record,
        Record::ToolResult { call: recovered_call, .. } if recovered_call == &call
    )));
    let result = recovered.iter().find_map(|entry| match &entry.record {
        Record::TurnCompleted {
            result: Some(result),
            ..
        } => Some(result),
        _ => None,
    });
    let result = result.expect("terminal result committed during hydrate");
    assert_eq!(result.call_id.to_string(), call);
    assert_eq!(result.value, input);
    assert_eq!(result.metadata.get("recovered"), Some(&"true".into()));

    journal
        .release(&session_id, &resident.st.lease)
        .await
        .expect("release recovered lease");
    drop(resident);
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn customer_terminal_before_brain_crash_replays_without_reexecuting_the_effect() {
    let data_dir = std::env::temp_dir().join(format!(
        "brain-customer-recovery-{}-{}",
        std::process::id(),
        crate::wall_ms()
    ));
    std::fs::create_dir_all(&data_dir).unwrap();
    let journal = Journal::new_memory("brain-customer-crashed");
    let transport = crate::customer::CustomerTransportConfig::new(
        "ws://127.0.0.1:3210/v1/customer-environment/socket",
        "http://127.0.0.1:3210",
    )
    .unwrap();
    let crashed = Brain::with_parts_and_services(
        BrainConfig {
            external_call_timeout: Duration::from_secs(2),
            idle_discard: Duration::from_secs(300),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(crate::adapter::DisabledToolExecutor),
        BrainServices {
            customer_transport: Some(transport.clone()),
            ..BrainServices::default()
        },
        crate::provider::fake::unscripted_factory(),
    );
    let created = crashed
        .create_session(
            typed_create(json!({
                "model":{"provider":"anthropic","name":"customer-recovery","api_key":"sk-test"},
                "client":{"id":"app","submit_retries":1},
                "environments":{"app":{
                    "extension":"test/app",
                    "protocol":"environment/v1",
                    "profile":{"kind":"callbacks","network":"unrestricted","recovery":"connection"},
                    "configuration":{"id":"app"}
                }},
                "tools":{"items":[{
                    "definition":{
                        "name":"lookup", "description":"lookup",
                        "contract_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "input_schema":{"type":"object"}, "output_schema":{"type":"object"}
                    },
                    "executor":{
                        "kind":"environment",
                        "environment":"app",
                        "callback_registration":"lookup",
                        "requirements":{}
                    }
                }]}
            })),
            None,
        )
        .await
        .unwrap();
    let session_id = created.id.to_string();
    let crashed_customer = crashed.customer.as_ref().unwrap().clone();
    let (crashed_grant, mut crashed_socket, crashed_epoch) =
        connect_customer_process(&crashed_customer, "process:stable").await;

    let mut head = journal.claim(&session_id).await.unwrap();
    let turn = "trn_customercrash0000000".to_owned();
    let call = "op_customercrash".to_owned();
    let input = json!({"id":7});
    let intent = crashed_customer
        .prepare_operation(
            "local",
            "app",
            &session_id,
            &call,
            "lookup",
            "lookup",
            &"a".repeat(64),
            input.clone(),
            crate::wall_ms() + 5_000,
        )
        .await
        .unwrap();
    head.doc.state = SessionLifecycle::Open;
    head.doc.turn = Some(turn.clone());
    head.doc.active_phase = Some(TurnPhase::ReadyToDispatchTools);
    head.doc.active_rounds = 1;
    head.doc.active_tool_calls = 1;
    head.doc.turns += 1;
    let first_seq = head.last_seq + 1;
    let records = vec![
        (
            first_seq,
            Record::UserMessage {
                turn: turn.clone(),
                content: vec![ContentBlock::text("look up id 7")],
                starts_turn: false,
                metadata: HashMap::new(),
                idempotency_key_hash: None,
                request_hash: None,
            },
        ),
        (first_seq + 1, Record::TurnStarted { turn: turn.clone() }),
        (
            first_seq + 2,
            Record::Assistant {
                turn: turn.clone(),
                agent: "root".into(),
                attempt_id: "att_customercrash0000000".into(),
                content: vec![ContentBlock::ToolUse {
                    id: call.clone(),
                    name: "lookup".into(),
                    input: input.clone(),
                }],
                stop: crate::message::StopReason::ToolUse,
            },
        ),
        (
            first_seq + 3,
            Record::CustomerCallIntent {
                turn: turn.clone(),
                call: call.clone(),
                client_id: intent.client_id.clone(),
                process_id: intent.process_id.clone(),
                request_digest: intent.request_digest.clone(),
                deadline_at_ms: intent.deadline_at_ms,
            },
        ),
        (
            first_seq + 4,
            Record::ToolCall {
                turn: turn.clone(),
                agent: "root".into(),
                call: call.clone(),
                name: "lookup".into(),
                input,
                detach: false,
            },
        ),
    ];
    let mut lease = Lease {
        fence: head.fence,
        last_seq: head.last_seq,
        retention: head.retention,
    };
    journal
        .commit(&session_id, &mut lease, &records, &head.doc, first_seq + 4)
        .await
        .unwrap();

    // The application runs the effect once and publishes its terminal, but Brain crashes
    // before the ToolResult decision. The application process retains that exact fact.
    let first_execution = {
        let customer = crashed_customer.clone();
        let intent = intent.clone();
        tokio::spawn(async move {
            customer
                .execute_prepared(intent, 0, CancellationToken::new())
                .await
        })
    };
    let Some(crate::customer::CustomerCommand::Offer(first_offer)) = crashed_socket.recv().await
    else {
        panic!("first customer offer")
    };
    crashed_customer
        .observation(
            &crashed_grant.grant_id,
            &crashed_grant.observation_token,
            crate::customer::CustomerObservation::Receipt {
                epoch: crashed_epoch,
                operation_id: first_offer.operation_id.clone(),
                request_digest: first_offer.request_digest.clone(),
                replayed: false,
            },
        )
        .await
        .unwrap();
    crashed_customer
        .observation(
            &crashed_grant.grant_id,
            &crashed_grant.observation_token,
            crate::customer::CustomerObservation::Terminal {
                epoch: crashed_epoch,
                operation_id: first_offer.operation_id,
                request_digest: first_offer.request_digest,
                ok: true,
                output: Some(json!({"value":7})),
                error: None,
            },
        )
        .await
        .unwrap();
    let uncommitted = first_execution.await.unwrap();
    assert!(uncommitted.terminal_receipt.is_some());
    journal.release(&session_id, &lease).await.unwrap();
    drop(crashed);

    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script([Scripted::Text("done after replay".into())]);
    let provider = fake.clone();
    let recovering = Brain::with_parts_and_services(
        BrainConfig {
            external_call_timeout: Duration::from_secs(2),
            idle_discard: Duration::from_secs(300),
            ..BrainConfig::default()
        },
        journal.cloned_as("brain-customer-recovering"),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(crate::adapter::DisabledToolExecutor),
        BrainServices {
            customer_transport: Some(transport),
            ..BrainServices::default()
        },
        Arc::new(move |_| provider.clone()),
    );
    let recovering_customer = recovering.customer.as_ref().unwrap().clone();
    let (replay_grant, mut replay_socket, replay_epoch) =
        connect_customer_process(&recovering_customer, "process:stable").await;
    let hydration = {
        let recovering = recovering.clone();
        let session_id = session_id.clone();
        tokio::spawn(async move { hydrate(&recovering, &session_id).await })
    };
    let Some(crate::customer::CustomerCommand::Offer(replay_offer)) = replay_socket.recv().await
    else {
        panic!("replayed customer offer")
    };
    assert_eq!(replay_offer.operation_id, call);
    assert_eq!(replay_offer.request_digest, intent.request_digest);
    // Retained application terminal replay: no handler/effect runs a second time.
    recovering_customer
        .observation(
            &replay_grant.grant_id,
            &replay_grant.observation_token,
            crate::customer::CustomerObservation::Terminal {
                epoch: replay_epoch,
                operation_id: replay_offer.operation_id,
                request_digest: replay_offer.request_digest,
                ok: true,
                output: Some(json!({"value":7})),
                error: None,
            },
        )
        .await
        .unwrap();
    let Some(crate::customer::CustomerCommand::Ack {
        operation_id,
        request_digest,
        ..
    }) = replay_socket.recv().await
    else {
        panic!("post-commit customer ack")
    };
    assert_eq!(operation_id, call);
    assert_eq!(request_digest, intent.request_digest);
    let resident = tokio::time::timeout(Duration::from_secs(3), hydration)
        .await
        .expect("customer recovery completed")
        .unwrap()
        .unwrap();
    assert!(resident.st.head.pending_customer_acks.is_empty());
    assert!(resident.st.head.turn.is_none());
    assert_eq!(fake.call_count.load(Ordering::Relaxed), 1);
    let recovered = journal
        .read_records(&session_id, first_seq + 4)
        .await
        .unwrap();
    assert_eq!(
        recovered
            .iter()
            .filter(|entry| matches!(entry.record, Record::ToolResult { ref call, .. } if call == &operation_id))
            .count(),
        1
    );
    assert!(
        recovered
            .iter()
            .any(|entry| matches!(entry.record, Record::CustomerTerminalReceived { .. }))
    );
    assert!(
        recovered
            .iter()
            .any(|entry| matches!(entry.record, Record::CustomerTerminalAcknowledged { .. }))
    );
    journal
        .release(&session_id, &resident.st.lease)
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn journaled_customer_terminal_is_reacked_after_process_restart() {
    let data_dir = std::env::temp_dir().join(format!(
        "brain-customer-ack-recovery-{}-{}",
        std::process::id(),
        crate::wall_ms()
    ));
    std::fs::create_dir_all(&data_dir).unwrap();
    let journal = Journal::new_memory("brain-customer-ack-crashed");
    let transport = crate::customer::CustomerTransportConfig::new(
        "ws://127.0.0.1:3210/v1/customer-environment/socket",
        "http://127.0.0.1:3210",
    )
    .unwrap();
    let crashed = Brain::with_parts_and_services(
        BrainConfig::default(),
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(crate::adapter::DisabledToolExecutor),
        BrainServices {
            customer_transport: Some(transport.clone()),
            ..BrainServices::default()
        },
        crate::provider::fake::unscripted_factory(),
    );
    let created = crashed
        .create_session(
            typed_create(json!({
                "model":{"provider":"anthropic","name":"unused","api_key":"sk-test"},
                "client":{"id":"app"}
            })),
            None,
        )
        .await
        .unwrap();
    let session_id = created.id.to_string();
    let mut head = journal.claim(&session_id).await.unwrap();
    let call = "op_ackrestart0000".to_owned();
    let request_digest = "a".repeat(64);
    let terminal_digest = "b".repeat(64);
    head.doc
        .pending_customer_acks
        .push(crate::journal::CustomerTerminalAckDoc {
            turn: "trn_ackrestart00000000".into(),
            call: call.clone(),
            client_id: "app".into(),
            process_id: "process:stable".into(),
            request_digest: request_digest.clone(),
            terminal_digest: terminal_digest.clone(),
        });
    let seq = head.last_seq + 1;
    let mut lease = Lease {
        fence: head.fence,
        last_seq: head.last_seq,
        retention: head.retention,
    };
    journal
        .commit(
            &session_id,
            &mut lease,
            &[(
                seq,
                Record::CustomerTerminalReceived {
                    turn: "trn_ackrestart00000000".into(),
                    call: call.clone(),
                    client_id: "app".into(),
                    process_id: "process:stable".into(),
                    request_digest: request_digest.clone(),
                    terminal_digest: terminal_digest.clone(),
                },
            )],
            &head.doc,
            seq,
        )
        .await
        .unwrap();
    journal.release(&session_id, &lease).await.unwrap();
    drop(crashed);

    let recovering = Brain::with_parts_and_services(
        BrainConfig::default(),
        journal.cloned_as("brain-customer-ack-recovering"),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(crate::adapter::DisabledToolExecutor),
        BrainServices {
            customer_transport: Some(transport),
            ..BrainServices::default()
        },
        crate::provider::fake::unscripted_factory(),
    );
    let customer = recovering.customer.as_ref().unwrap().clone();
    let (_, mut socket, epoch) = connect_customer_process(&customer, "process:stable").await;
    let hydration = {
        let recovering = recovering.clone();
        let session_id = session_id.clone();
        tokio::spawn(async move { hydrate(&recovering, &session_id).await })
    };
    let Some(crate::customer::CustomerCommand::Ack {
        epoch: ack_epoch,
        operation_id,
        request_digest: ack_request,
        terminal_digest: ack_terminal,
    }) = socket.recv().await
    else {
        panic!("durable ack replay")
    };
    assert_eq!(ack_epoch, epoch);
    assert_eq!(operation_id, call);
    assert_eq!(ack_request, request_digest);
    assert_eq!(ack_terminal, terminal_digest);
    let resident = hydration.await.unwrap().unwrap();
    assert!(resident.st.head.pending_customer_acks.is_empty());
    let records = journal.read_records(&session_id, seq).await.unwrap();
    assert!(
        records
            .iter()
            .any(|entry| matches!(entry.record, Record::CustomerTerminalAcknowledged { .. }))
    );
    journal
        .release(&session_id, &resident.st.lease)
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn managed_submit_unknown_survives_cleanup_crash_without_resubmission() {
    assert_managed_submit_unknown_recovery(false).await;
}

#[tokio::test]
async fn cancelled_managed_submit_stays_cancelled_across_cleanup_crash() {
    assert_managed_submit_unknown_recovery(true).await;
}

async fn assert_managed_submit_unknown_recovery(cancellation_requested: bool) {
    let case = if cancellation_requested {
        "brain-managed-submit-cancelled"
    } else {
        "brain-managed-submit-unknown"
    };
    let journal = Journal::new_memory(case);
    let ports = Arc::new(UnknownManagedPorts::default());
    ports.fail_next_dematerialize.store(true, Ordering::Release);
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    if !cancellation_requested {
        fake.script([Scripted::Text("continued after an honest unknown".into())]);
    }
    let provider = fake.clone();
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            idle_discard: Duration::from_secs(300),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(crate::adapter::DisabledToolExecutor),
        BrainServices {
            bundle_storage: Some(Arc::new(TestBundleStorage)),
            environments: test_environment_registry(
                "test.managed",
                ports.clone(),
                ports.clone(),
                Some(ports.clone()),
            ),
            ..BrainServices::default()
        },
        Arc::new(move |_| provider.clone()),
    );
    let created = brain
        .create_session(
            typed_create(json!({
                "model":{"provider":"anthropic","name":"managed-unknown","api_key":"key"}
            })),
            Some("managed-submit-unknown"),
        )
        .await
        .expect("create unknown-submit recovery session");
    let session_id = created.id.to_string();
    let mut resident = hydrate(&brain, &session_id)
        .await
        .expect("claim initial crash fold");

    let turn = "trn_managedunknown000000".to_owned();
    let call = "op_managedunknown0000".to_owned();
    let name = "managed_unknown_test".to_owned();
    let bundle_digest = "a".repeat(64);
    declare_test_managed_environment(&mut resident.st.head, &name, &bundle_digest);
    let binding_ref = "bnd_managedunknown0000";
    let input = json!({"effect":"already_may_have_run"});
    let mut envelope: brain_protocol::environment::OperationEnvelope =
        serde_json::from_value(json!({
            "operation_id":call,
            "request_digest":"0".repeat(64),
            "session_id":session_id,
            "root_id":resident.st.head.root_id,
            "turn_id":turn,
            "caller_id":"agent_root",
            "fence":resident.st.lease.fence,
            "generation":null,
            "binding_ref":binding_ref,
            "capability":name,
            "input":{"kind":"inline","value":input},
            "phase":"execute",
            "target_ref":null,
            "deadline_at_ms":crate::wall_ms() + 60_000,
            "resources":managed_environment_resources().unwrap(),
            "network":sealed_sandbox_network(&resident.st.head).unwrap(),
            "trace":{}
        }))
        .expect("valid managed operation envelope");
    envelope.request_digest = brain_protocol::contract::operation_request_digest(&envelope);
    let binding: brain_protocol::environment::ResolvedBinding = serde_json::from_value(json!({
        "binding_ref":binding_ref,
        "capabilities":["execution","session_preparation"],
        "environment_id":"environment_managedunknown",
        "limits":{
            "max_inline_input_bytes":brain_protocol::MAX_MANAGED_TOOL_INPUT_BYTES,
            "max_inline_result_bytes":brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
            "max_wait_ms":1
        },
        "recovery":"retained"
    }))
    .expect("valid resolved binding");
    resident.managed_bindings = Arc::new(HashMap::from([(
        name.clone(),
        crate::environment::ManagedBinding {
            environment_name: "workspace".into(),
            resolved: binding,
            environment: ports.clone(),
        },
    )]));
    resident.st.head.state = SessionLifecycle::Open;
    resident.st.head.turn = Some(turn.clone());
    resident.st.head.active_phase = Some(if cancellation_requested {
        TurnPhase::ManagedCancelling
    } else {
        TurnPhase::ManagedRunning
    });
    resident.st.head.active_rounds = 1;
    resident.st.head.active_tool_calls = 1;
    resident.st.head.turns += 1;
    let first_seq = resident.st.take_seq();
    let turn_started_seq = resident.st.take_seq();
    let assistant_seq = resident.st.take_seq();
    let tool_call_seq = resident.st.take_seq();
    let managed_intent_seq = resident.st.take_seq();
    let records = vec![
        (
            first_seq,
            Record::UserMessage {
                turn: turn.clone(),
                content: vec![ContentBlock::text("run the managed effect")],
                starts_turn: false,
                metadata: HashMap::new(),
                idempotency_key_hash: None,
                request_hash: None,
            },
        ),
        (turn_started_seq, Record::TurnStarted { turn: turn.clone() }),
        (
            assistant_seq,
            Record::Assistant {
                turn: turn.clone(),
                agent: "root".into(),
                attempt_id: "att_managedunknown00000".into(),
                content: vec![ContentBlock::ToolUse {
                    id: call.clone(),
                    name: name.clone(),
                    input: input.clone(),
                }],
                stop: crate::message::StopReason::ToolUse,
            },
        ),
        (
            tool_call_seq,
            Record::ToolCall {
                turn: turn.clone(),
                agent: "root".into(),
                call: call.clone(),
                name: name.clone(),
                input,
                detach: false,
            },
        ),
        (
            managed_intent_seq,
            Record::ManagedCallIntent {
                turn: turn.clone(),
                call: call.clone(),
                name: name.clone(),
                envelope,
            },
        ),
    ];
    commit(&brain, &session_id, &mut resident.st, records)
        .await
        .expect("commit crash-after-Submit intent");

    let crash_entries = journal.read_records(&session_id, 0).await.unwrap();
    assert_eq!(
        pending_managed(&crash_entries).unwrap().len(),
        1,
        "the simulated crash leaves one durable managed intent; records={:?}",
        crash_entries
            .iter()
            .map(|entry| (entry.seq, entry.record.kind_name()))
            .collect::<Vec<_>>()
    );
    let error = recover_managed_calls(&brain, &session_id, &mut resident, &crash_entries)
        .await
        .expect_err("inject a crash boundary after the unknown marker and status commit");
    assert!(
        matches!(error, BrainError::EnvironmentUnavailable(_)),
        "{error:?}"
    );
    assert_eq!(ports.submits.load(Ordering::Acquire), 1);
    let after_unknown = journal.read_records(&session_id, 0).await.unwrap();
    assert_eq!(
        after_unknown
            .iter()
            .filter(|entry| matches!(entry.record, Record::ManagedCallUnknown { .. }))
            .count(),
        1,
        "OperationUnknown is a single durable revocation of submit replay"
    );
    assert!(after_unknown.iter().any(|entry| matches!(
        &entry.record,
        Record::EnvironmentChanged { environment: _, status }
            if status.reason.as_ref().is_some_and(|reason| reason.as_str() == MANAGED_UNKNOWN_SANDBOX_REASON)
                && status.generation.as_ref().is_some_and(|generation| generation.as_str() == "gen_unknown_submit")
                && status.target_ref.as_ref().is_some_and(|target_ref| target_ref.as_str() == "tgt_unknown_submit")
                && status.expires_at_ms.is_some()
    )));
    journal
        .release(&session_id, &resident.st.lease)
        .await
        .expect("release simulated crashed recovery owner");
    drop(resident);
    drop(brain);

    let provider = fake.clone();
    let recovering = Brain::with_parts_and_services(
        BrainConfig {
            idle_discard: Duration::from_secs(300),
            ..BrainConfig::default()
        },
        journal.cloned_as(format!("{case}-restart")),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(crate::adapter::DisabledToolExecutor),
        BrainServices {
            bundle_storage: Some(Arc::new(TestBundleStorage)),
            environments: test_environment_registry(
                "test.managed",
                ports.clone(),
                ports.clone(),
                Some(ports.clone()),
            ),
            ..BrainServices::default()
        },
        Arc::new(move |_| provider.clone()),
    );
    let recovered = hydrate(&recovering, &session_id)
        .await
        .expect("restart consumes the unknown marker without Submit");
    assert_eq!(
        ports.submits.load(Ordering::Acquire),
        1,
        "the journaled unknown marker permanently forbids a second Submit"
    );
    assert_eq!(ports.dematerialize_calls.load(Ordering::Acquire), 2);
    assert_eq!(
        recovered
            .st
            .head
            .environment_targets
            .get("workspace")
            .expect("reconciled environment")
            .state,
        brain_protocol::environment::SandboxState::Terminated
    );
    assert!(recovered.st.head.turn.is_none());
    let final_records = journal.read_records(&session_id, 0).await.unwrap();
    assert_eq!(
        final_records
            .iter()
            .filter(|entry| matches!(
                &entry.record,
                Record::ToolResult { call: result_call, outcome, .. }
                    if result_call == &call && *outcome == ToolOutcome::Interrupted
            ))
            .count(),
        1
    );
    let expected_stop = if cancellation_requested {
        TurnStopReason::Cancelled
    } else {
        TurnStopReason::EndTurn
    };
    assert_eq!(
        final_records
            .iter()
            .filter(|entry| matches!(
                &entry.record,
                Record::TurnCompleted { stop_reason, .. } if *stop_reason == expected_stop
            ))
            .count(),
        1,
        "the recovered turn must preserve its requested terminal disposition"
    );
    fake.assert_drained(
        u64::from(!cancellation_requested),
        "managed OperationUnknown recovery",
    )
    .unwrap();
    recovering
        .journal
        .release(&session_id, &recovered.st.lease)
        .await
        .unwrap();
}

#[tokio::test]
async fn ending_session_reconciles_stale_managed_intent_without_resubmission() {
    let journal = Journal::new_memory("brain-stale-managed-ending");
    let ports = Arc::new(UnknownManagedPorts::default());
    ports.fail_next_dematerialize.store(true, Ordering::Release);
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    let provider = fake.clone();
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            idle_discard: Duration::from_secs(300),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(crate::adapter::DisabledToolExecutor),
        BrainServices {
            bundle_storage: Some(Arc::new(TestBundleStorage)),
            environments: test_environment_registry(
                "test.managed",
                ports.clone(),
                ports.clone(),
                Some(ports.clone()),
            ),
            ..BrainServices::default()
        },
        Arc::new(move |_| provider.clone()),
    );
    let created = brain
        .create_session(
            typed_create(json!({
                "model":{"provider":"anthropic","name":"stale-managed","api_key":"key"}
            })),
            Some("stale-managed-ending"),
        )
        .await
        .expect("create stale managed recovery session");
    let session_id = created.id.to_string();
    let mut resident = hydrate(&brain, &session_id)
        .await
        .expect("claim stale managed session");
    let turn = "trn_stalemanaged0000000".to_owned();
    let call = "op_stalemanaged000000".to_owned();
    let name = "managed_stale_test".to_owned();
    let bundle_digest = "a".repeat(64);
    declare_test_managed_environment(&mut resident.st.head, &name, &bundle_digest);
    let mut envelope: brain_protocol::environment::OperationEnvelope =
        serde_json::from_value(json!({
            "operation_id":call,
            "request_digest":"0".repeat(64),
            "session_id":session_id,
            "root_id":resident.st.head.root_id,
            "turn_id":turn,
            "caller_id":"agent_root",
            "fence":resident.st.lease.fence,
            "generation":null,
            "binding_ref":"bnd_stalemanaged0000",
            "capability":name,
            "input":{"kind":"inline","value":{"effect":"may_have_started"}},
            "phase":"execute",
            "target_ref":null,
            "deadline_at_ms":crate::wall_ms() + 60_000,
            "resources":managed_environment_resources().unwrap(),
            "network":sealed_sandbox_network(&resident.st.head).unwrap(),
            "trace":{}
        }))
        .expect("valid stale managed envelope");
    envelope.request_digest = brain_protocol::contract::operation_request_digest(&envelope);

    resident.st.head.turn = Some(turn.clone());
    resident.st.head.active_phase = Some(TurnPhase::ManagedRunning);
    resident.st.head.active_rounds = 1;
    resident.st.head.active_tool_calls = 1;
    let intent_records = vec![
        (
            resident.st.take_seq(),
            Record::TurnStarted { turn: turn.clone() },
        ),
        (
            resident.st.take_seq(),
            Record::ToolCall {
                turn: turn.clone(),
                agent: "root".into(),
                call: call.clone(),
                name: name.clone(),
                input: json!({"effect":"may_have_started"}),
                detach: false,
            },
        ),
        (
            resident.st.take_seq(),
            Record::ManagedCallIntent {
                turn: turn.clone(),
                call: call.clone(),
                name: name.clone(),
                envelope,
            },
        ),
    ];
    commit(&brain, &session_id, &mut resident.st, intent_records)
        .await
        .expect("commit managed intent");

    resident.st.head.turn = None;
    resident.st.head.active_phase = None;
    resident.st.head.active_rounds = 0;
    resident.st.head.active_tool_calls = 0;
    let failed_records = vec![
        (
            resident.st.take_seq(),
            Record::TurnFailed {
                turn: turn.clone(),
                code: "internal".into(),
                message: "sandbox capacity is exhausted".into(),
                details: None,
            },
        ),
        (
            resident.st.take_seq(),
            Record::State {
                state: SessionLifecycle::Open,
                turn: None,
            },
        ),
    ];
    commit(&brain, &session_id, &mut resident.st, failed_records)
        .await
        .expect("commit failed turn without a managed result");
    resident.st.head.ended = true;
    resident.st.head.state = SessionLifecycle::Ending;
    let ending = vec![(
        resident.st.take_seq(),
        Record::State {
            state: SessionLifecycle::Ending,
            turn: None,
        },
    )];
    commit(&brain, &session_id, &mut resident.st, ending)
        .await
        .expect("commit ending lifecycle");

    let crash_entries = journal.read_records(&session_id, 0).await.unwrap();
    let error = recover_managed_calls(&brain, &session_id, &mut resident, &crash_entries)
        .await
        .expect_err("inject cleanup loss after submit replay is revoked");
    assert!(
        matches!(error, BrainError::EnvironmentUnavailable(_)),
        "{error:?}"
    );
    assert_eq!(ports.submits.load(Ordering::Acquire), 0);
    journal
        .release(&session_id, &resident.st.lease)
        .await
        .expect("release simulated cleanup crash owner");
    drop(resident);
    drop(brain);

    let provider = fake.clone();
    let recovering = Brain::with_parts_and_services(
        BrainConfig {
            idle_discard: Duration::from_secs(300),
            ..BrainConfig::default()
        },
        journal.cloned_as("brain-stale-managed-ending-restart"),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(crate::adapter::DisabledToolExecutor),
        BrainServices {
            bundle_storage: Some(Arc::new(TestBundleStorage)),
            environments: test_environment_registry(
                "test.managed",
                ports.clone(),
                ports.clone(),
                Some(ports.clone()),
            ),
            ..BrainServices::default()
        },
        Arc::new(move |_| provider.clone()),
    );
    let recovered = hydrate(&recovering, &session_id)
        .await
        .expect("restart reconciles the stale managed intent");
    assert_eq!(ports.submits.load(Ordering::Acquire), 0);
    assert_eq!(ports.dematerialize_calls.load(Ordering::Acquire), 2);
    assert_eq!(recovered.st.head.state, SessionLifecycle::Ending);
    assert!(recovered.st.head.turn.is_none());
    let records = journal.read_records(&session_id, 0).await.unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|entry| matches!(entry.record, Record::ManagedCallUnknown { .. }))
            .count(),
        1
    );
    assert_eq!(
        records
            .iter()
            .filter(|entry| matches!(
                &entry.record,
                Record::ToolResult { call: result_call, outcome, .. }
                    if result_call == &call && *outcome == ToolOutcome::Interrupted
            ))
            .count(),
        1
    );
    assert!(pending_managed(&records).unwrap().is_empty());

    let mut resident = Some(recovered);
    assert!(
        continue_end_session(&recovering, &session_id, &mut resident)
            .await
            .expect("ending cleanup converges")
    );
    assert_eq!(
        journal.get_head(&session_id).await.unwrap().doc.state,
        SessionLifecycle::Ended
    );
    if let Some(resident) = resident {
        recovering
            .journal
            .release(&session_id, &resident.st.lease)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn deleting_managed_session_hydrates_without_repreparing_environment_definitions() {
    let journal = Journal::new_memory("brain-deleting-managed-hydrate");
    let brain = Brain::with_parts_and_services(
        BrainConfig::default(),
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices::default(),
        crate::provider::fake::unscripted_factory(),
    );
    let created = brain
        .create_session(
            typed_create(json!({
                "model":{"provider":"anthropic","name":"deleting-managed","api_key":"key"}
            })),
            Some("deleting-managed-hydrate"),
        )
        .await
        .expect("create deleting managed session");
    let session_id = created.id.to_string();
    let mut resident = hydrate(&brain, &session_id)
        .await
        .expect("claim deleting managed session");
    let bundle_digest = "a".repeat(64);
    resident.st.head.prefix.managed_bundles.push(
        serde_json::from_value(json!({
            "bundle_digest":bundle_digest,
            "bytes":1,
            "contract_digest":"b".repeat(64),
            "layers":[{
                "digest":bundle_digest,
                "bytes":1,
                "media_type":"application/javascript+esm",
                "mount_path":"/tool/runtime.mjs",
                "unpack":"file",
                "object":{
                    "bytes":1,
                    "media_type":"application/javascript+esm",
                    "object_id":format!("bundle_{bundle_digest}"),
                    "sha256":bundle_digest,
                },
            }],
            "required_env":[],
            "target":"linux-amd64",
            "execute_path":"/tool/runtime.mjs",
            "setup_path":null,
            "environment_name":"workspace",
            "tool_name":"managed_delete_test",
        }))
        .expect("valid managed bundle descriptor"),
    );
    resident.st.head.state = SessionLifecycle::Deleting;
    resident.st.head.ended = true;
    resident.st.head.turn = None;
    resident.st.head.active_phase = None;
    let state_seq = resident.st.take_seq();
    commit(
        &brain,
        &session_id,
        &mut resident.st,
        vec![(
            state_seq,
            Record::State {
                state: SessionLifecycle::Deleting,
                turn: None,
            },
        )],
    )
    .await
    .expect("commit deleting lifecycle with a managed descriptor");
    journal
        .release(&session_id, &resident.st.lease)
        .await
        .expect("release deleting session before cold hydration");
    drop(resident);

    let recovered = hydrate(&brain, &session_id)
        .await
        .expect("deleting hydration must not require or recreate Environment definitions");
    assert_eq!(recovered.st.head.state, SessionLifecycle::Deleting);
    assert!(!recovered.st.head.prefix.managed_bundles.is_empty());
    assert!(recovered.managed_bindings.is_empty());
    journal
        .release(&session_id, &recovered.st.lease)
        .await
        .expect("release recovered deleting session");
}

async fn simulate_provider_only_crash(
    retries: u32,
    attempt_state: ProviderAttemptState,
) -> (Arc<Brain>, Journal, Arc<FakeProvider>, String, u64, PathBuf) {
    let data_dir = std::env::temp_dir().join(format!(
        "brain-provider-recovery-{}-{}-{retries}",
        std::process::id(),
        crate::wall_ms()
    ));
    std::fs::create_dir_all(&data_dir).expect("create provider recovery data dir");
    let journal = Journal::new_memory(format!("brain-provider-recovery-{retries}"));
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script([Scripted::Text("replacement completed".into())]);
    let provider = fake.clone();
    let provider_factory: ProviderFactory = Arc::new(move |_| provider.clone());
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            idle_discard: Duration::from_secs(300),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices::default(),
        provider_factory,
    );
    let created = brain
        .create_session(
            typed_create_result(json!({
                "model": {
                    "provider": "anthropic",
                    "name": "provider-recovery-test",
                    "api_key": "sk-test"
                },
                "provider_recovery_retries": retries
            }))
            .expect("valid provider recovery create"),
            None,
        )
        .await
        .expect("create provider recovery session");
    let session_id = created.id.to_string();
    for _ in 0..100 {
        if journal
            .get_head(&session_id)
            .await
            .expect("recovery head")
            .last_seq
            >= 2
        {
            break;
        }
        tokio::task::yield_now().await;
    }

    let mut head = journal.claim(&session_id).await.expect("claim crash owner");
    let turn = "trn_providercrash00000000".to_owned();
    let content = vec![ContentBlock::text("recover provider-only phase")];
    let history = vec![Message {
        role: crate::message::Role::User,
        content: content.clone(),
    }];
    let (prefix, _) = build_prefix(&head.doc.prefix, 512).expect("rebuild sealed prefix");
    let request = fake
        .build_request(
            &prefix,
            &history,
            &ProviderKey::new("sk-test"),
            head.doc.prefix.base_url.as_deref().expect("base URL"),
        )
        .expect("build crashed request");
    let request_digest = crate::turn::model_request_digest(&request);
    let logical_operation_id = "mdl_providercrash00000000".to_owned();
    let attempt_id = "att_providercrash00000000".to_owned();
    head.doc.state = SessionLifecycle::Open;
    head.doc.turn = Some(turn.clone());
    head.doc.active_phase = Some(if attempt_state == ProviderAttemptState::Intent {
        TurnPhase::ModelIntentCommitted
    } else {
        TurnPhase::ModelRunning
    });
    head.doc.provider_attempt = Some(crate::journal::ProviderAttemptDoc {
        logical_operation_id: logical_operation_id.clone(),
        attempt_id: attempt_id.clone(),
        request_digest: request_digest.clone(),
        state: attempt_state,
        replacements_used: 0,
    });
    head.doc.active_context = HashMap::new();
    head.doc.active_rounds = 0;
    head.doc.active_tool_calls = 0;
    head.doc.turns += 1;
    let first_seq = head.last_seq + 1;
    let records = vec![
        (
            first_seq,
            Record::UserMessage {
                turn: turn.clone(),
                content,
                starts_turn: false,
                metadata: HashMap::new(),
                idempotency_key_hash: None,
                request_hash: None,
            },
        ),
        (first_seq + 1, Record::TurnStarted { turn: turn.clone() }),
        (
            first_seq + 2,
            Record::ModelCallIntent {
                turn: turn.clone(),
                logical_operation_id,
                attempt_id,
                request_digest,
                replacement: 0,
            },
        ),
    ];
    let mut lease = Lease {
        fence: head.fence,
        last_seq: head.last_seq,
        retention: head.retention,
    };
    journal
        .commit(&session_id, &mut lease, &records, &head.doc, first_seq + 2)
        .await
        .expect("commit simulated provider crash");
    journal
        .defer_recovery(&session_id)
        .await
        .expect("release failed provider owner and schedule recovery");
    (brain, journal, fake, session_id, first_seq + 2, data_dir)
}

#[tokio::test]
async fn provider_only_crash_replaces_the_same_logical_request_by_default() {
    for state in [ProviderAttemptState::Intent, ProviderAttemptState::Running] {
        let (brain, journal, fake, session_id, crash_seq, data_dir) =
            simulate_provider_only_crash(1, state).await;
        let resident = hydrate(&brain, &session_id)
            .await
            .expect("hydrate provider-only crash");
        assert_eq!(fake.call_count.load(Ordering::Relaxed), 1);
        assert_eq!(resident.st.head.state, SessionLifecycle::Open);
        assert!(resident.st.head.turn.is_none());
        let records = journal
            .read_records(&session_id, crash_seq)
            .await
            .expect("read provider recovery records");
        assert!(records.iter().any(|entry| matches!(
            &entry.record,
            Record::ModelCallUnknown {
                possibly_duplicated: true,
                ..
            }
        )));
        assert!(records.iter().any(|entry| matches!(
            &entry.record,
            Record::ModelCallIntent { replacement: 1, .. }
        )));
        assert!(
            records
                .iter()
                .any(|entry| matches!(&entry.record, Record::ModelCallCompleted { .. }))
        );
        journal
            .release(&session_id, &resident.st.lease)
            .await
            .expect("release recovered provider lease");
        drop(resident);
        let _ = std::fs::remove_dir_all(data_dir);
    }
}

#[tokio::test]
async fn provider_only_crash_with_zero_retries_commits_honest_interruption() {
    let (brain, journal, fake, session_id, crash_seq, data_dir) =
        simulate_provider_only_crash(0, ProviderAttemptState::Running).await;
    let resident = hydrate(&brain, &session_id)
        .await
        .expect("hydrate provider-only crash");
    assert_eq!(fake.call_count.load(Ordering::Relaxed), 0);
    assert_eq!(resident.st.head.state, SessionLifecycle::Open);
    assert!(resident.st.head.turn.is_none());
    let records = journal
        .read_records(&session_id, crash_seq)
        .await
        .expect("read strict interruption records");
    assert!(records.iter().any(|entry| matches!(
        &entry.record,
        Record::ModelCallUnknown {
            possibly_duplicated: true,
            ..
        }
    )));
    assert!(records.iter().any(|entry| matches!(
        &entry.record,
        Record::TurnCompleted { stop_reason, .. } if *stop_reason == TurnStopReason::Interrupted
    )));
    assert!(!records.iter().any(|entry| matches!(
        &entry.record,
        Record::ModelCallIntent { replacement: 1, .. }
    )));
    journal
        .release(&session_id, &resident.st.lease)
        .await
        .expect("release interrupted provider lease");
    drop(resident);
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn recovery_worker_resumes_provider_only_crash_without_customer_traffic() {
    let (_crashed_brain, journal, fake, session_id, crash_seq, data_dir) =
        simulate_provider_only_crash(1, ProviderAttemptState::Running).await;
    let provider = fake.clone();
    let recovering = Brain::with_parts_and_services(
        BrainConfig {
            recovery_poll_interval: Duration::from_millis(5),
            recovery_shards_per_poll: crate::journal::RECOVERY_SHARDS,
            recovery_page_size: 16,
            idle_discard: Duration::from_secs(300),
            ..BrainConfig::default()
        },
        journal.cloned_as("brain-recovery-worker"),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices::default(),
        Arc::new(move |_| provider.clone()),
    );
    recovering.start_recovery_worker();

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let head = journal.get_head(&session_id).await.unwrap();
            if head.doc.turn.is_none() && head.last_seq > crash_seq {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("due recovery completed without a follow-up request");
    assert_eq!(fake.call_count.load(Ordering::Relaxed), 1);
    let records = journal.read_records(&session_id, crash_seq).await.unwrap();
    assert!(
        records
            .iter()
            .any(|entry| matches!(entry.record, Record::ModelCallIntent { replacement: 1, .. }))
    );
    assert!(
        records
            .iter()
            .any(|entry| matches!(entry.record, Record::TurnCompleted { .. }))
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

async fn run_live_provider_case(
    retries: u32,
    script: Vec<Scripted>,
) -> (Journal, Arc<FakeProvider>, String, PathBuf) {
    let data_dir = std::env::temp_dir().join(format!(
        "brain-live-provider-recovery-{}-{}-{retries}",
        std::process::id(),
        crate::wall_ms()
    ));
    std::fs::create_dir_all(&data_dir).expect("create live provider recovery dir");
    let journal = Journal::new_memory(format!("brain-live-provider-{retries}"));
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script(script);
    let provider = fake.clone();
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            idle_discard: Duration::from_secs(300),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices::default(),
        Arc::new(move |_| provider.clone()),
    );
    let created = brain
        .create_session(
            typed_create_result(json!({
                "model": {"provider":"anthropic", "name":"live-recovery", "api_key":"sk-test"},
                "provider_recovery_retries": retries
            }))
            .unwrap(),
            None,
        )
        .await
        .unwrap();
    let session_id = created.id.to_string();
    let (_, admitted_seq) = brain
        .message(
            &session_id,
            serde_json::from_value(json!("exercise recovery")).unwrap(),
        )
        .await
        .unwrap();
    // Generous budget: live-retry cases sleep through jittered backoff before finishing.
    for _ in 0..2_500 {
        let head = journal.get_head(&session_id).await.unwrap();
        if head.doc.turn.is_none() && head.last_seq > admitted_seq {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let head = journal.get_head(&session_id).await.unwrap();
    assert!(
        head.doc.turn.is_none(),
        "live provider turn remained active"
    );
    (journal, fake, session_id, data_dir)
}

#[tokio::test]
async fn live_unknown_before_or_after_stream_bytes_uses_the_same_replacement_budget() {
    for partial_text in [None, Some("provisional bytes".to_owned())] {
        let (journal, fake, session_id, data_dir) = run_live_provider_case(
            1,
            vec![
                Scripted::TransportError {
                    partial_text,
                    message: "ambiguous reset".into(),
                },
                Scripted::Text("replacement completed".into()),
            ],
        )
        .await;
        assert_eq!(fake.call_count.load(Ordering::Relaxed), 2);
        let records = journal.read_records(&session_id, 0).await.unwrap();
        let intents = records
            .iter()
            .filter_map(|entry| match &entry.record {
                Record::ModelCallIntent {
                    logical_operation_id,
                    request_digest,
                    replacement,
                    ..
                } => Some((logical_operation_id, request_digest, replacement)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(intents.len(), 2);
        assert_eq!(intents[0].0, intents[1].0);
        assert_eq!(intents[0].1, intents[1].1);
        assert_eq!(*intents[1].2, 1);
        assert!(
            records
                .iter()
                .any(|entry| matches!(entry.record, Record::ModelCallCompleted { .. }))
        );
        let _ = std::fs::remove_dir_all(data_dir);
    }
}

#[tokio::test]
async fn live_unknown_zero_or_exhausted_budget_interrupts_honestly() {
    for (retries, script, expected_calls) in [
        (
            0,
            vec![Scripted::TransportError {
                partial_text: None,
                message: "strict reset".into(),
            }],
            1,
        ),
        (
            1,
            vec![
                Scripted::TransportError {
                    partial_text: None,
                    message: "first reset".into(),
                },
                Scripted::TransportError {
                    partial_text: Some("then reset".into()),
                    message: "second reset".into(),
                },
            ],
            2,
        ),
    ] {
        let (journal, fake, session_id, data_dir) = run_live_provider_case(retries, script).await;
        assert_eq!(fake.call_count.load(Ordering::Relaxed), expected_calls);
        let records = journal.read_records(&session_id, 0).await.unwrap();
        assert!(records.iter().any(|entry| matches!(
            &entry.record,
            Record::TurnCompleted { stop_reason, .. } if *stop_reason == TurnStopReason::Interrupted
        )));
        assert_eq!(
            records
                .iter()
                .filter(|entry| matches!(entry.record, Record::ModelCallUnknown { .. }))
                .count(),
            expected_calls as usize
        );
        let _ = std::fs::remove_dir_all(data_dir);
    }
}

#[tokio::test]
async fn loop_bundles_are_verified_before_registry_admission() {
    struct RejectRegistry;
    impl crate::agentloop::AgentloopRegistry for RejectRegistry {
        fn resolve(
            &self,
            _selector: &crate::journal::AgentloopSelectorDoc,
        ) -> Result<Arc<dyn crate::agentloop::Agentloop>> {
            Err(BrainError::Invalid("loop not enabled".into()))
        }
    }
    let journal = Journal::new_memory("agentloop-selector");
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    let provider = fake.clone();
    let brain = Brain::with_parts_and_services(
        BrainConfig::default(),
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices {
            agentloop_registry: Some(Arc::new(RejectRegistry)),
            ..BrainServices::default()
        },
        Arc::new(move |_| provider.clone()),
    );
    let model = json!({"provider":"anthropic", "name":"selector-test", "api_key":"sk-test"});

    let bundle = b"export function activate() { return \"{}\" }";
    let encoded = {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(bundle)
    };
    let digest = hex::encode(Sha256::digest(bundle));
    let model_component = b"test model";
    let model_digest = hex::encode(Sha256::digest(model_component));
    let wrong_digest = brain
        .create_session(
            typed_create(json!({"model": model, "agentloop": {
                "component_digest": "0".repeat(64),
                "world": "aex:agentloop/agentloop@1.0.0"
            }, "component_artifacts": [
                {"component_digest": model_digest, "component_base64": base64::engine::general_purpose::STANDARD.encode(model_component), "bytes": model_component.len()},
                {"component_digest": "0".repeat(64), "component_base64": encoded, "bytes": bundle.len()}
            ]})),
            None,
        )
        .await;
    assert!(
        matches!(&wrong_digest, Err(BrainError::Invalid(message)) if message.contains("digest")),
        "a bundle that does not match its declared digest never reaches the registry"
    );
    let custom = brain
        .create_session(
            typed_create(json!({"model": model, "agentloop": {
                "component_digest": digest,
                "world": "aex:agentloop/agentloop@1.0.0"
            }, "component_artifacts": [
                {"component_digest": model_digest, "component_base64": base64::engine::general_purpose::STANDARD.encode(model_component), "bytes": model_component.len()},
                {"component_digest": digest, "component_base64": encoded, "bytes": bundle.len()}
            ]})),
            None,
        )
        .await;
    assert!(
        matches!(&custom, Err(BrainError::Invalid(message)) if message.contains("not enabled")),
        "a composition that cannot admit the loop refuses create: {custom:?}"
    );
}

#[tokio::test]
async fn tool_components_are_verified_and_admitted_before_session_commit() {
    struct TestToolRegistry(AtomicUsize);

    #[async_trait::async_trait]
    impl crate::tools::ToolRegistry for TestToolRegistry {
        fn admit(
            &self,
            component_digest: &str,
            world: &str,
            component: &[u8],
            config: &serde_json::Map<String, serde_json::Value>,
            grants: &[String],
            environment: Option<&str>,
        ) -> Result<crate::journal::ToolSelectorDoc> {
            self.0.fetch_add(1, Ordering::Relaxed);
            assert_eq!(world, crate::tools::TOOL_WORLD);
            assert_eq!(component_digest, hex::encode(Sha256::digest(component)));
            assert_eq!(config["mode"], "echo");
            assert_eq!(grants, ["journal"]);
            assert_eq!(environment, None);
            Ok(crate::journal::ToolSelectorDoc {
                component_digest: component_digest.into(),
                component_bytes: component.len() as u64,
                world: world.into(),
                config: config.clone(),
                grants: grants.to_vec(),
                environment: None,
            })
        }

        async fn invoke(
            &self,
            _selector: &crate::journal::ToolSelectorDoc,
            _request: crate::tools::ComponentToolRequest,
            _capabilities: Arc<dyn crate::tools::ToolCapabilityHandler>,
        ) -> Result<CallOutcome> {
            unreachable!("create only admits the component")
        }
    }

    let registry = Arc::new(TestToolRegistry(AtomicUsize::new(0)));
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    let provider = fake.clone();
    let brain = Brain::with_parts_and_services(
        BrainConfig::default(),
        Journal::new_memory("tool-component-admission"),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices {
            tool_registry: Some(registry.clone()),
            ..BrainServices::default()
        },
        Arc::new(move |_| provider.clone()),
    );
    let tool_component = b"test tool";
    let tool_digest = hex::encode(Sha256::digest(tool_component));
    let mut request = serde_json::to_value(typed_create(json!({
        "model": {"provider":"anthropic", "name":"tool-test", "api_key":"sk-test"},
        "tools": {"items": [{
            "definition": {
                "name": "echo",
                "contract_digest": "a".repeat(64),
                "input_schema": {"type":"object"},
                "output_schema": {"type":"object"}
            },
            "executor": {
                "kind": "component",
                "component_digest": tool_digest,
                "world": "aex:tool/tool@1.0.0",
                "config": {"mode":"echo"},
                "grants": ["journal"]
            }
        }]}
    })))
    .unwrap();
    request["component_artifacts"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "component_digest": tool_digest,
            "component_base64": base64::engine::general_purpose::STANDARD.encode(tool_component),
            "bytes": tool_component.len()
        }));
    brain
        .create_session(serde_json::from_value(request).unwrap(), None)
        .await
        .unwrap();
    assert_eq!(registry.0.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn a_composition_registry_resolves_a_sealed_loop() {
    struct TestRegistry;
    impl crate::agentloop::AgentloopRegistry for TestRegistry {
        fn resolve(
            &self,
            _selector: &crate::journal::AgentloopSelectorDoc,
        ) -> Result<Arc<dyn crate::agentloop::Agentloop>> {
            Ok(Arc::new(crate::agentloop::SequentialAgentloop))
        }
        fn admit(
            &self,
            component_digest: &str,
            world: &str,
            component: &[u8],
            config: &serde_json::Map<String, serde_json::Value>,
        ) -> Result<crate::journal::AgentloopSelectorDoc> {
            Ok(crate::journal::AgentloopSelectorDoc {
                component_digest: component_digest.into(),
                component_bytes: component.len() as u64,
                world: world.into(),
                config: config.clone(),
            })
        }
    }
    let journal = Journal::new_memory("agentloop-registry");
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script([Scripted::Text("resolved answer".into())]);
    let provider = fake.clone();
    let brain = Brain::with_parts_and_services(
        BrainConfig::default(),
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(crate::adapter::DisabledToolExecutor),
        BrainServices {
            agentloop_registry: Some(Arc::new(TestRegistry)),
            ..BrainServices::default()
        },
        Arc::new(move |_| provider.clone() as Arc<dyn crate::provider::Provider>),
    );
    let bundle = b"test loop";
    let model_component = b"test model";
    let created = brain
        .create_session(
            typed_create(json!({
                "model": {"provider":"anthropic", "name":"registry-test", "api_key":"sk-test"},
                "component_artifacts": [
                    {
                        "component_digest": hex::encode(Sha256::digest(model_component)),
                        "component_base64": base64::engine::general_purpose::STANDARD.encode(model_component),
                        "bytes": model_component.len()
                    },
                    {
                        "component_digest": hex::encode(Sha256::digest(bundle)),
                        "component_base64": base64::engine::general_purpose::STANDARD.encode(bundle),
                        "bytes": bundle.len()
                    }
                ],
                "agentloop": {
                    "component_digest": hex::encode(Sha256::digest(bundle)),
                    "world": "aex:agentloop/agentloop@1.0.0"
                }
            })),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&created).unwrap()["agentloop"]["world"],
        "aex:agentloop/agentloop@1.0.0",
        "the registry's admitted identity seals"
    );
    let session_id = created.id.to_string();
    let (_, admitted_seq) = brain
        .message(
            &session_id,
            serde_json::from_value(json!("drive the sealed loop")).unwrap(),
        )
        .await
        .unwrap();
    for _ in 0..1_000 {
        let head = journal.get_head(&session_id).await.unwrap();
        if head.doc.turn.is_none() && head.last_seq > admitted_seq {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let records = journal.read_records(&session_id, 0).await.unwrap();
    assert!(
        records.iter().any(|entry| matches!(
            &entry.record,
            Record::TurnCompleted { stop_reason, .. } if *stop_reason == TurnStopReason::EndTurn
        )),
        "the per-session loop resolved at turn time and drove the turn"
    );
}

#[tokio::test]
async fn draining_refuses_new_work_while_admitted_turns_finish() {
    let journal = Journal::new_memory("brain-drain");
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script([Scripted::Text("x".repeat(800))]);
    // Paced emission gives the turn a real duration so the drain window is observable.
    fake.tokens_per_second.store(400, Ordering::Relaxed);
    let provider = fake.clone();
    let brain = Brain::with_parts_and_services(
        BrainConfig::default(),
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices::default(),
        Arc::new(move |_| provider.clone()),
    );
    let created = brain
        .create_session(
            typed_create_result(json!({
                "model": {"provider":"anthropic", "name":"drain-test", "api_key":"sk-test"}
            }))
            .unwrap(),
            None,
        )
        .await
        .unwrap();
    let session_id = created.id.to_string();
    brain
        .message(
            &session_id,
            serde_json::from_value(json!("one slow answer")).unwrap(),
        )
        .await
        .unwrap();
    assert!(brain.active_turns() > 0, "the turn holds its permit");

    brain.begin_drain();
    let refused = brain
        .message(
            &session_id,
            serde_json::from_value(json!("refused")).unwrap(),
        )
        .await;
    assert!(matches!(refused, Err(BrainError::Draining)));
    let refused_create = brain
        .create_session(
            typed_create_result(json!({
                "model": {"provider":"anthropic", "name":"drain-test", "api_key":"sk-test"}
            }))
            .unwrap(),
            None,
        )
        .await;
    assert!(matches!(refused_create, Err(BrainError::Draining)));

    // The admitted turn is never interrupted: it runs to its durable terminal.
    for _ in 0..2_000 {
        if brain.active_turns() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(brain.active_turns(), 0, "the admitted turn drained");
    let records = journal.read_records(&session_id, 0).await.unwrap();
    assert!(records.iter().any(|entry| matches!(
        &entry.record,
        Record::TurnCompleted { stop_reason, .. } if *stop_reason == TurnStopReason::EndTurn
    )));
}

#[tokio::test]
async fn clean_provider_failures_retry_in_place_without_replacement_budget() {
    // Replacement budget ZERO: live retry is a separate mechanism from the durable
    // digest-identical replacement path and must not consume it.
    let (journal, fake, session_id, data_dir) = run_live_provider_case(
        0,
        vec![
            Scripted::ProviderStatus {
                status: 500,
                body: "transient upstream failure".into(),
                retry_after_ms: None,
            },
            Scripted::ProviderStatus {
                status: 429,
                body: "rate limited".into(),
                retry_after_ms: Some(100),
            },
            Scripted::Text("recovered in place".into()),
        ],
    )
    .await;
    assert_eq!(fake.call_count.load(Ordering::Relaxed), 3);
    let records = journal.read_records(&session_id, 0).await.unwrap();
    assert!(records.iter().any(|entry| matches!(
        &entry.record,
        Record::TurnCompleted { stop_reason, .. } if *stop_reason == TurnStopReason::EndTurn
    )));
    assert!(
        !records
            .iter()
            .any(|entry| matches!(entry.record, Record::ModelCallUnknown { .. })),
        "a complete HTTP error response is definitive, never an unknown outcome"
    );
    assert_eq!(
        records
            .iter()
            .filter(|entry| matches!(entry.record, Record::ModelCallIntent { .. }))
            .count(),
        1,
        "in-place retries reuse the committed intent"
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn persistent_clean_failures_exhaust_live_retries_and_fail_honestly() {
    let script = (0..4)
        .map(|_| Scripted::ProviderStatus {
            status: 503,
            body: "unavailable".into(),
            retry_after_ms: Some(50),
        })
        .collect();
    let (journal, fake, session_id, data_dir) = run_live_provider_case(1, script).await;
    assert_eq!(
        fake.call_count.load(Ordering::Relaxed),
        4,
        "one attempt plus exactly three live retries"
    );
    let records = journal.read_records(&session_id, 0).await.unwrap();
    assert!(records.iter().any(|entry| matches!(
        &entry.record,
        Record::TurnFailed { code, .. } if code == "provider_error"
    )));
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn quota_exhaustion_fails_fast_despite_the_429_status() {
    let (journal, fake, session_id, data_dir) = run_live_provider_case(
        1,
        vec![Scripted::ProviderStatus {
            status: 429,
            body: r#"{"error":{"code":"insufficient_quota"}}"#.into(),
            retry_after_ms: Some(50),
        }],
    )
    .await;
    assert_eq!(fake.call_count.load(Ordering::Relaxed), 1);
    let records = journal.read_records(&session_id, 0).await.unwrap();
    assert!(records.iter().any(|entry| matches!(
        &entry.record,
        Record::TurnFailed { code, .. } if code == "provider_error"
    )));
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn deterministic_provider_4xx_is_not_unknown_or_retried() {
    let (journal, fake, session_id, data_dir) = run_live_provider_case(
        2,
        vec![
            Scripted::ProviderStatus {
                status: 400,
                body: "invalid model request".into(),
                retry_after_ms: None,
            },
            Scripted::Text("must not run".into()),
        ],
    )
    .await;
    assert_eq!(fake.call_count.load(Ordering::Relaxed), 1);
    let records = journal.read_records(&session_id, 0).await.unwrap();
    assert!(
        !records
            .iter()
            .any(|entry| matches!(entry.record, Record::ModelCallUnknown { .. }))
    );
    assert!(
        records
            .iter()
            .any(|entry| matches!(entry.record, Record::TurnFailed { .. }))
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

/// Echo executor answering every engine tool call with a payload big enough that a few
/// tool rounds exceed a minimum-size context window.
struct BigEcho;

#[async_trait::async_trait]
impl ToolExecutor for BigEcho {
    fn supports(&self, capability: &str) -> bool {
        capability == "bench.echo"
    }

    async fn call(
        &self,
        _capability: &str,
        _request: ExternalToolCallRequest,
        _cancel: CancellationToken,
    ) -> Result<ExternalToolCallResponse> {
        Ok(serde_json::from_value(json!({
            "outcome": "completed",
            "content": "x".repeat(3200),
            "is_error": false,
            "disposition": "continue",
            "result": {"data": "x".repeat(3200)},
        }))
        .expect("echo response"))
    }
}

async fn compaction_session(journal: &Journal, fake: Arc<FakeProvider>) -> (Arc<Brain>, String) {
    let mut cfg = BrainConfig {
        idle_discard: Duration::from_secs(300),
        ..BrainConfig::default()
    };
    cfg.official_capabilities.insert(
        "brain.bench_echo".into(),
        crate::config::ServerToolPolicy {
            capability: "bench.echo".into(),
            scope: brain_protocol::session::ExternalToolScope::All,
            completion: brain_protocol::session::ExternalToolCompletion::Continue,
            effect: brain_protocol::session::ExternalToolEffect::ReplaySafe,
            max_input_bytes: brain_protocol::MAX_EXTERNAL_TOOL_INPUT_BYTES,
        },
    );
    let provider = fake.clone();
    let brain = Brain::with_parts_and_services(
        cfg,
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(BigEcho),
        BrainServices::default(),
        Arc::new(move |_| provider.clone() as Arc<dyn Provider>),
    );
    let created = brain
        .create_session(
            typed_create_result(json!({
                "model": {"provider":"anthropic", "name":"compact", "api_key":"sk-test",
                          "context_window_tokens": 8192, "max_output_tokens": 64},
                "tools": {"items": [{
                    "definition": {
                        "name": "bash",
                        "description": "echo tool",
                        "contract_digest": "a".repeat(64),
                        "input_schema": {"type":"object","additionalProperties":true},
                        "output_schema": {"type":"object","additionalProperties":true}
                    },
                    "executor": {"kind":"engine","capability":"brain.bench_echo"}
                }]}
            }))
            .unwrap(),
            None,
        )
        .await
        .unwrap();
    let session_id = created.id.to_string();
    (brain, session_id)
}

async fn run_one_turn(brain: &Arc<Brain>, journal: &Journal, session_id: &str) {
    let (_, admitted) = brain
        .message(session_id, serde_json::from_value(json!("go")).unwrap())
        .await
        .unwrap();
    for _ in 0..1000 {
        let head = journal.get_head(session_id).await.unwrap();
        if head.doc.turn.is_none() && head.last_seq > admitted {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("the turn never reached a terminal");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_spawned_child_whose_first_turn_spawns_again_never_deadlocks() {
    // The r4 dev wedge: a child session starts with its first turn already active, and
    // that turn is driven during actor hydration. When the child's model answered with a
    // subagents spawn, the intrinsic delivered a command to the child's own actor — which
    // was busy driving the turn — and the tree deadlocked; END then queued forever behind
    // it. The spawn intrinsic now creates the grandchild directly, no self-delivery.
    let journal = Journal::new_memory("brain-child-self-spawn");
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script([
        // Only the child's turn consumes scripts: round one spawns a grandchild with a
        // mid-turn fork of the child itself, round two answers.
        Scripted::tool(
            "subagents",
            json!({
                "action": "spawn_agent", "task_name": "grandchild", "message": "grand prompt",
                "fork_turns": "1"
            }),
        ),
        Scripted::Text("child done".into()),
        Scripted::Text("grandchild done".into()),
    ]);
    let provider = fake.clone();
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            idle_discard: Duration::from_secs(300),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(crate::adapter::DisabledToolExecutor),
        BrainServices::default(),
        Arc::new(move |_| provider.clone() as Arc<dyn Provider>),
    );
    let created = brain
        .create_session(
            typed_create_result(json!({
                "model": {"provider":"anthropic","name":"m","api_key":"sk-test"},
                "tools": {"items": [{
                    "definition": {
                        "name":"subagents", "description":"children",
                        "contract_digest": "d".repeat(64),
                        "input_schema": {"type":"object","additionalProperties":true},
                        "output_schema": {"type":"object","additionalProperties":true}
                    },
                    "executor": {"kind":"engine", "capability":"brain.subagents"}
                }]}
            }))
            .unwrap(),
            None,
        )
        .await
        .unwrap();
    let root_id = created.id.to_string();
    let child = brain
        .create_child(&root_id, "run".into(), Some("child".into()), None, None)
        .await
        .unwrap();
    let child_id = serde_json::to_value(&child).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let settled = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let head = journal.get_head(&child_id).await.unwrap();
            if head.doc.turn.is_none() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await;
    assert!(
        settled.is_ok(),
        "the child's first turn deadlocked on its own spawn delivery"
    );
    let (grandchildren, _) = brain.list_children(&child_id, None, 10).await.unwrap();
    assert_eq!(grandchildren.len(), 1, "the grandchild exists");
    let records = journal.read_records(&child_id, 0).await.unwrap();
    let failure = records.iter().find_map(|entry| match &entry.record {
        Record::TurnFailed { code, message, .. } => Some(format!("{code}: {message}")),
        _ => None,
    });
    assert!(failure.is_none(), "child turn failed: {}", failure.unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_later_turn_still_sees_earlier_turns_verbatim() {
    // The r4 continuation canary regression: per-turn summary marks replaced real history
    // with the previous answer's text, so "remember CEDAR" vanished from turn two's
    // context. Rebuilds now replay the journal tail; the exact user words must reach the
    // provider on the later turn.
    let journal = Journal::new_memory("brain-continuation");
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script([
        Scripted::Text("noted".into()),
        Scripted::Text("the word is CEDAR".into()),
    ]);
    let provider = fake.clone();
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            idle_discard: Duration::from_secs(300),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(crate::adapter::DisabledToolExecutor),
        BrainServices::default(),
        Arc::new(move |_| provider.clone() as Arc<dyn Provider>),
    );
    let created = brain
        .create_session(
            typed_create_result(json!({
                "model": {"provider":"anthropic","name":"m","api_key":"sk-test"}
            }))
            .unwrap(),
            None,
        )
        .await
        .unwrap();
    let session_id = created.id.to_string();
    for message in ["Remember the word CEDAR.", "What was the word?"] {
        let (_, admitted) = brain
            .message(&session_id, serde_json::from_value(json!(message)).unwrap())
            .await
            .unwrap();
        for _ in 0..1000 {
            let head = journal.get_head(&session_id).await.unwrap();
            if head.doc.turn.is_none() && head.last_seq > admitted {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }
    let arrivals = fake.arrivals.lock().expect("arrivals");
    assert_eq!(arrivals.len(), 2);
    assert_eq!(
        arrivals[1].message_count, 3,
        "turn two must carry turn one's user message and answer verbatim          (a summary-mark rebuild would collapse them to 2 messages)"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gateway_style_model_names_cross_the_loop_contract() {
    // Hosted models arrive as gateway paths ("openai/gpt-4.1-nano"); the loop contract's
    // ModelName admits them where a plain Identifier does not. The 2026-08-23 r3 canaries
    // failed every turn on exactly this projection.
    let journal = Journal::new_memory("brain-model-name");
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script([Scripted::Text("answered".into())]);
    let provider = fake.clone();
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            idle_discard: Duration::from_secs(300),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(crate::adapter::DisabledToolExecutor),
        BrainServices::default(),
        Arc::new(move |_| provider.clone() as Arc<dyn Provider>),
    );
    let created = brain
        .create_session(
            typed_create_result(json!({
                "model": {"provider":"openai_compatible", "name":"openai/gpt-4.1-nano",
                          "api_key":"sk-test", "base_url":"https://gateway.example/v1"}
            }))
            .unwrap(),
            None,
        )
        .await
        .unwrap();
    let session_id = created.id.to_string();
    run_one_turn(&brain, &journal, &session_id).await;
    let records = journal.read_records(&session_id, 0).await.unwrap();
    let failure = records.iter().find_map(|entry| match &entry.record {
        Record::TurnFailed { code, message, .. } => Some(format!("{code}: {message}")),
        _ => None,
    });
    assert!(failure.is_none(), "turn failed: {}", failure.unwrap());
    assert!(records.iter().any(|entry| matches!(
        &entry.record,
        Record::TurnCompleted { stop_reason, .. } if *stop_reason == TurnStopReason::EndTurn
    )));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn self_compaction_summarizes_installs_a_mark_and_continues() {
    // The aex loop owns compaction: tool rounds accumulate past the sealed context
    // window mid-turn, the loop summarizes everything but a recent tail through the
    // sealed model (twice here, the first continuation still over budget), and the
    // turn completes. This drives the policy through the whole kernel.
    let journal = Journal::new_memory("brain-self-compaction");
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script([
        Scripted::tool("bash", json!({})),
        Scripted::tool("bash", json!({})),
        Scripted::Text("summary one keeps id=alpha".into()),
        Scripted::Text("summary two keeps id=alpha".into()),
        Scripted::Text("completed after compaction".into()),
    ]);
    let (brain, session_id) = compaction_session(&journal, fake.clone()).await;
    run_one_turn(&brain, &journal, &session_id).await;
    let records = journal.read_records(&session_id, 0).await.unwrap();
    let failure = records.iter().find_map(|entry| match &entry.record {
        Record::TurnFailed { code, message, .. } => Some(format!("{code}: {message}")),
        _ => None,
    });
    assert!(failure.is_none(), "turn failed: {}", failure.unwrap());
    assert_eq!(
        fake.call_count.load(Ordering::Relaxed),
        5,
        "two tool rounds, two summarization rounds, one continuation"
    );
    assert!(
        !records
            .iter()
            .any(|entry| matches!(&entry.record, Record::LoopMark { .. })),
        "the loop writes no per-turn marks; rebuilds replay the journal tail"
    );
    assert!(
        records.iter().any(|entry| matches!(
            &entry.record,
            Record::TurnCompleted { stop_reason, .. } if *stop_reason == TurnStopReason::EndTurn
        )),
        "the turn completes normally after compaction"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_clean_summarization_failure_fails_the_turn_honestly() {
    // Same over-budget turn, but the summarization round dies on a clean provider
    // rejection: the loop declares turn_fail and the honest provider error is journaled.
    let journal = Journal::new_memory("brain-self-compaction-fail");
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    fake.script([
        Scripted::tool("bash", json!({})),
        Scripted::tool("bash", json!({})),
        Scripted::ProviderStatus {
            status: 400,
            body: "bad compaction request".into(),
            retry_after_ms: None,
        },
    ]);
    let (brain, session_id) = compaction_session(&journal, fake.clone()).await;
    run_one_turn(&brain, &journal, &session_id).await;
    let records = journal.read_records(&session_id, 0).await.unwrap();
    assert!(
        records.iter().any(|entry| matches!(
            &entry.record,
            Record::TurnFailed { code, .. } if code == "provider_error"
        )),
        "the failed summarization surfaces as an honest provider error"
    );
}

fn declared_tool(host: &str) -> crate::config::ToolDecl {
    crate::config::ToolDecl {
        name: "fetcher".into(),
        description: "fetches".into(),
        contract_digest: "b".repeat(64),
        input_schema: json!({"type":"object"}),
        output_schema: json!({"type":"object"}),
        route: crate::config::ToolRoute::Customer {
            environment: "app".into(),
            registration: "reg".into(),
        },
        network_needs: vec![json!({"host": host, "ports": [443], "protocol": "tls"})],
    }
}

fn network_policy(value: serde_json::Value) -> brain_protocol::session::NetworkPolicy {
    serde_json::from_value(value).expect("network policy parses the public contract")
}

#[test]
fn tool_network_declarations_merge_into_the_sealed_allowlist() {
    let sealed = merge_session_network(None, &[declared_tool("api.example.com")]).expect("merged");
    assert_eq!(sealed["outbound"], "allowlist");
    assert_eq!(sealed["destinations"][0]["host"], "api.example.com");

    // Session allows union with declarations, deduplicated.
    let policy = network_policy(json!({
        "outbound": "allowlist",
        "destinations": [
            {"host": "api.example.com", "ports": [443], "protocol": "tls"},
            {"host": "cdn.example.com", "ports": [443], "protocol": "tls"}
        ]
    }));
    let sealed =
        merge_session_network(Some(&policy), &[declared_tool("api.example.com")]).expect("merged");
    let hosts: Vec<&str> = sealed["destinations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|destination| destination["host"].as_str().unwrap())
        .collect();
    assert_eq!(hosts, ["api.example.com", "cdn.example.com"]);
}

#[test]
fn a_session_deny_beats_a_tool_declaration() {
    let policy = network_policy(json!({"outbound": "none", "deny": ["api.example.com"]}));
    let sealed =
        merge_session_network(Some(&policy), &[declared_tool("api.example.com")]).expect("merged");
    assert_eq!(sealed, json!({"outbound": "none"}));

    // A wildcard deny removes the whole subtree; an empty allowlist result is refused.
    let policy = network_policy(json!({
        "outbound": "allowlist",
        "destinations": [{"host": "a.svc.example.com", "ports": [443], "protocol": "tls"}],
        "deny": ["*.example.com"]
    }));
    let error = merge_session_network(Some(&policy), &[]).expect_err("empty after denies");
    assert!(error.to_string().contains("empty after"), "{error}");
}

#[test]
fn the_kernel_does_not_embed_product_specific_network_denials() {
    let sealed = merge_session_network(None, &[declared_tool("control.product.invalid")])
        .expect("composition-specific policy belongs outside the kernel");
    assert_eq!(sealed["outbound"], "allowlist");
}

#[test]
fn deny_rules_are_incompatible_with_public_outbound() {
    let policy = network_policy(json!({"outbound": "public", "deny": ["evil.example.com"]}));
    let error = merge_session_network(Some(&policy), &[]).expect_err("unenforceable");
    assert!(error.to_string().contains("gateway path"), "{error}");
}

#[tokio::test]
async fn tenant_storage_quota_rejection_restores_the_live_actor_fold() {
    let data_dir = std::env::temp_dir().join(format!(
        "brain-storage-tenant-quota-{}-{}",
        std::process::id(),
        crate::wall_ms()
    ));
    std::fs::create_dir_all(&data_dir).expect("create tenant quota data dir");
    let journal = Journal::new_memory("brain-storage-tenant-quota");
    let storage = Arc::new(ReservationStorage::new(journal.clone()));
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            idle_discard: Duration::from_secs(300),
            storage_max_object_bytes: 8,
            storage_max_session_bytes: 20,
            storage_max_tenant_bytes: 8,
            storage_transfer_ttl: Duration::from_secs(60 * 60),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(crate::adapter::DisabledToolExecutor),
        BrainServices {
            session_storage: Some(storage.clone()),
            ..BrainServices::default()
        },
        crate::provider::fake::unscripted_factory(),
    );
    let create = || {
        typed_create_result(json!({
            "model": {
                "provider": "anthropic",
                "name": "storage-quota-test",
                "api_key": "sk-test"
            }
        }))
        .expect("valid storage quota create")
    };
    let first = brain
        .create_session(create(), None)
        .await
        .expect("create first root");
    let second = brain
        .create_session(create(), None)
        .await
        .expect("create second root");
    brain
        .storage_prepare_upload(
            &first.id,
            crate::storage::StorageUploadIntent {
                key: "full.bin".into(),
                bytes: 8,
                sha256: Some("a".repeat(64)),
                content_type: None,
                overwrite: false,
            },
        )
        .await
        .expect("first root consumes tenant quota");

    for _ in 0..2 {
        let error = brain
            .storage_prepare_upload(
                &second.id,
                crate::storage::StorageUploadIntent {
                    key: "rejected.bin".into(),
                    bytes: 1,
                    sha256: Some("b".repeat(64)),
                    content_type: None,
                    overwrite: false,
                },
            )
            .await
            .expect_err("the same live actor must re-run tenant admission on retry");
        assert!(matches!(
            error,
            BrainError::TenantStorageQuotaExceeded {
                requested: 1,
                limit: 8
            }
        ));
    }
    assert_eq!(
        storage.prepares.load(Ordering::Relaxed),
        1,
        "a rejected resident reservation must never reach the storage adapter"
    );
    let rejected = journal
        .get_head(&second.id)
        .await
        .expect("authoritative rejected root");
    assert_eq!(rejected.doc.session_storage_bytes, 0);
    assert_eq!(rejected.doc.storage_reserved_bytes, 0);
    assert_eq!(rejected.doc.tenant_metered_storage_bytes, 0);
    assert!(rejected.doc.storage_upload.is_none());
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn deep_ancestor_end_fence_discards_the_mutated_resident_before_retry() {
    let data_dir = std::env::temp_dir().join(format!(
        "brain-deep-ancestor-fence-{}-{}",
        std::process::id(),
        crate::wall_ms()
    ));
    std::fs::create_dir_all(&data_dir).expect("create ancestor fence data dir");
    let journal = Journal::new_memory("brain-deep-ancestor-fence");
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    let provider = fake.clone();
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            idle_discard: Duration::from_secs(300),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(DisabledToolExecutor),
        BrainServices::default(),
        Arc::new(move |_| provider.clone()),
    );
    let root = brain
        .create_session(
            typed_create(json!({
                "model": {
                    "provider": "anthropic",
                    "name": "ancestor-fence-test",
                    "api_key": "sk-test"
                }
            })),
            Some("ancestor-fence-root"),
        )
        .await
        .expect("create ancestor fence root");
    let root_id = root.id.to_string();
    let root_head = journal.get_head(&root_id).await.expect("root head");

    let child_id = "ses_ancestorfencechild000";
    let mut child = root_head.doc.clone();
    child.root_id = root_id.clone();
    child.parent_id = Some(root_id.clone());
    child.ancestor_ids = vec![root_id.clone()];
    child.depth = 1;
    child.create_key_hash = None;
    child.create_request_hash = None;
    child.turn = None;
    child.turns = 0;
    child.last_seq = 1;
    child.context_fork = None;
    journal
        .create(
            child_id,
            &child,
            &Record::State {
                state: SessionLifecycle::Open,
                turn: None,
            },
        )
        .await
        .expect("create depth-one child");

    let grandchild_id = "ses_ancestorfencegrand000";
    let mut grandchild = child.clone();
    grandchild.parent_id = Some(child_id.into());
    grandchild.ancestor_ids = vec![root_id.clone(), child_id.into()];
    grandchild.depth = 2;
    journal
        .create(
            grandchild_id,
            &grandchild,
            &Record::State {
                state: SessionLifecycle::Open,
                turn: None,
            },
        )
        .await
        .expect("create depth-two child");

    // The descendant can claim and hydrate before the root fence. Its next admission mutates
    // a local TurnStarted projection, but the atomic journal decision must observe this root
    // transition and fence that stale resident.
    let _ = brain
        .cancel(grandchild_id)
        .await
        .expect("hydrate descendant before root fence");
    let mut fenced_root = journal.claim(&root_id).await.expect("claim root fence");
    fenced_root.doc.ended = true;
    fenced_root.doc.state = SessionLifecycle::Ending;
    let mut root_lease = Lease {
        fence: fenced_root.fence,
        last_seq: fenced_root.last_seq,
        retention: fenced_root.retention,
    };
    let root_fence_seq = fenced_root.last_seq + 1;
    journal
        .commit(
            &root_id,
            &mut root_lease,
            &[(
                root_fence_seq,
                Record::State {
                    state: SessionLifecycle::Ending,
                    turn: None,
                },
            )],
            &fenced_root.doc,
            root_fence_seq,
        )
        .await
        .expect("commit constant-size root admission fence");

    for attempt in 0..2 {
        let error = brain
            .message(
                grandchild_id,
                MessageRequestContent::String(format!("late message {attempt}").parse().unwrap()),
            )
            .await
            .expect_err("root fence rejects every deep admission");
        assert!(matches!(error, BrainError::Fenced));
        if attempt == 0 {
            tokio::time::timeout(Duration::from_secs(1), async {
                loop {
                    let closed = brain
                        .sessions
                        .lock()
                        .expect("session actors")
                        .get(grandchild_id)
                        .is_none_or(tokio::sync::mpsc::Sender::is_closed);
                    if closed {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("fenced descendant actor is reclaimed");
        }
    }
    let durable = journal
        .get_head(grandchild_id)
        .await
        .expect("durable descendant remains quiescent");
    assert_eq!(durable.last_seq, 1);
    assert_eq!(durable.doc.turns, 0);
    assert!(durable.doc.turn.is_none());
    assert_eq!(fake.call_count.load(Ordering::Relaxed), 0);
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn staged_overwrite_never_adopts_an_older_byte_identical_object() {
    let data_dir = std::env::temp_dir().join(format!(
        "brain-storage-provenance-staged-{}-{}",
        std::process::id(),
        crate::wall_ms()
    ));
    std::fs::create_dir_all(&data_dir).expect("create staged provenance data dir");
    let journal = Journal::new_memory("brain-storage-provenance-staged");
    let storage = Arc::new(ReservationStorage::new(journal.clone()));
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            idle_discard: Duration::from_secs(300),
            storage_max_object_bytes: 16,
            storage_max_session_bytes: 32,
            storage_max_tenant_bytes: 32,
            storage_transfer_ttl: Duration::from_secs(60 * 60),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(crate::adapter::DisabledToolExecutor),
        BrainServices {
            session_storage: Some(storage.clone()),
            ..BrainServices::default()
        },
        crate::provider::fake::unscripted_factory(),
    );
    let created = brain
        .create_session(
            typed_create(json!({
                "model": {
                    "provider": "anthropic",
                    "name": "staged-provenance-test",
                    "api_key": "sk-test"
                }
            })),
            None,
        )
        .await
        .expect("create staged provenance root");
    let session_id = created.id.to_string();
    let bytes = b"same";
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let old = brain
        .storage_write_inline(
            &session_id,
            "same.bin".into(),
            encoded,
            Some("text/plain".into()),
            false,
        )
        .await
        .expect("publish old object");
    let ticket = brain
        .storage_prepare_upload(
            &session_id,
            crate::storage::StorageUploadIntent {
                key: "same.bin".into(),
                bytes: bytes.len() as u64,
                sha256: Some(hex::encode(Sha256::digest(bytes))),
                content_type: Some("application/json".into()),
                overwrite: true,
            },
        )
        .await
        .expect("reserve byte-identical overwrite");

    assert!(matches!(
        brain
            .storage_complete_upload(&session_id, &ticket.transfer_id)
            .await,
        Err(BrainError::FileNotFound(_))
    ));
    brain
        .storage_reconcile(&session_id)
        .await
        .expect("a future reservation remains pending, not falsely completed");
    let pending = journal.get_head(&session_id).await.unwrap();
    assert!(pending.doc.storage_upload.as_ref().is_some_and(|upload| {
        upload.transfer_id == ticket.transfer_id && upload.state == UploadReservationState::Reserved
    }));
    let still_old = storage.stat(&session_id, "same.bin").await.unwrap();
    assert_eq!(still_old.publication_id, old.publication_id);
    assert_eq!(still_old.content_type.as_deref(), Some("text/plain"));

    // Even a buggy destination carrying the new publication id is not exact proof if its
    // sealed content type differs.
    storage
        .objects
        .lock()
        .expect("storage objects")
        .get_mut(&format!("{session_id}\0same.bin"))
        .expect("old object")
        .publication_id = Some(ticket.transfer_id.clone());
    assert!(matches!(
        brain
            .storage_complete_upload(&session_id, &ticket.transfer_id)
            .await,
        Err(BrainError::FileNotFound(_))
    ));
    let records = journal.read_records(&session_id, 0).await.unwrap();
    assert!(!records.iter().any(|entry| matches!(
        &entry.record,
        Record::StorageUploadPublished { transfer_id, .. }
            if transfer_id == &ticket.transfer_id
    )));

    storage
        .staged
        .lock()
        .expect("staged uploads")
        .insert(ticket.transfer_id.clone());
    let published = brain
        .storage_complete_upload(&session_id, &ticket.transfer_id)
        .await
        .expect("the exact staged transfer publishes");
    assert_eq!(
        published.publication_id.as_deref(),
        Some(ticket.transfer_id.as_str())
    );
    assert_eq!(published.content_type.as_deref(), Some("application/json"));
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn inline_overwrite_retry_reexecutes_after_pre_publication_crash() {
    let data_dir = std::env::temp_dir().join(format!(
        "brain-storage-provenance-inline-{}-{}",
        std::process::id(),
        crate::wall_ms()
    ));
    std::fs::create_dir_all(&data_dir).expect("create inline provenance data dir");
    let journal = Journal::new_memory("brain-storage-provenance-inline");
    let storage = Arc::new(ReservationStorage::new(journal.clone()));
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            idle_discard: Duration::from_secs(300),
            storage_max_object_bytes: 16,
            storage_max_session_bytes: 32,
            storage_max_tenant_bytes: 32,
            storage_transfer_ttl: Duration::from_secs(60 * 60),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(crate::adapter::DisabledToolExecutor),
        BrainServices {
            session_storage: Some(storage.clone()),
            ..BrainServices::default()
        },
        crate::provider::fake::unscripted_factory(),
    );
    let created = brain
        .create_session(
            typed_create(json!({
                "model": {
                    "provider": "anthropic",
                    "name": "inline-provenance-test",
                    "api_key": "sk-test"
                }
            })),
            None,
        )
        .await
        .expect("create inline provenance root");
    let session_id = created.id.to_string();
    let encoded = base64::engine::general_purpose::STANDARD.encode(b"same");
    let old = brain
        .storage_write_inline(
            &session_id,
            "same.bin".into(),
            encoded.clone(),
            Some("text/plain".into()),
            false,
        )
        .await
        .expect("publish old inline object");
    storage
        .fail_next_write_before_effect
        .store(true, Ordering::Relaxed);
    assert!(
        brain
            .storage_write_inline(
                &session_id,
                "same.bin".into(),
                encoded.clone(),
                Some("application/json".into()),
                true,
            )
            .await
            .is_err()
    );
    let reserved = journal.get_head(&session_id).await.unwrap();
    let intent_id = reserved
        .doc
        .storage_upload
        .as_ref()
        .filter(|upload| upload.state == UploadReservationState::InlineReserved)
        .expect("durable inline intent survives pre-effect crash")
        .transfer_id
        .clone();
    let unchanged = storage.stat(&session_id, "same.bin").await.unwrap();
    assert_eq!(unchanged.publication_id, old.publication_id);
    brain
        .storage_reconcile(&session_id)
        .await
        .expect("future inline intent remains pending");
    assert_eq!(
        journal
            .get_head(&session_id)
            .await
            .unwrap()
            .doc
            .storage_upload
            .as_ref()
            .map(|upload| upload.state.as_str()),
        Some("inline_reserved")
    );

    let retried = brain
        .storage_write_inline(
            &session_id,
            "same.bin".into(),
            encoded,
            Some("application/json".into()),
            true,
        )
        .await
        .expect("retry executes the unpublished inline intent");
    assert_eq!(retried.publication_id.as_deref(), Some(intent_id.as_str()));
    assert_eq!(retried.content_type.as_deref(), Some("application/json"));
    assert_eq!(storage.writes.load(Ordering::Relaxed), 3);
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn copied_upload_is_adopted_after_crash_and_expiry_without_customer_traffic() {
    let data_dir = std::env::temp_dir().join(format!(
        "brain-storage-copy-crash-{}-{}",
        std::process::id(),
        crate::wall_ms()
    ));
    std::fs::create_dir_all(&data_dir).expect("create copy crash data dir");
    let journal = Journal::new_memory("brain-storage-copy-crash");
    let storage = Arc::new(ReservationStorage::new(journal.clone()));
    let cfg = BrainConfig {
        recovery_poll_interval: Duration::from_millis(5),
        recovery_shards_per_poll: crate::journal::RECOVERY_SHARDS,
        recovery_page_size: 16,
        idle_discard: Duration::from_secs(300),
        storage_max_object_bytes: 8,
        storage_max_session_bytes: 20,
        storage_max_tenant_bytes: 8,
        storage_transfer_ttl: Duration::from_secs(60 * 60),
        ..BrainConfig::default()
    };
    let compose = |owner: &str| {
        Brain::with_parts_and_services(
            cfg.clone(),
            journal.cloned_as(owner),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(crate::adapter::DisabledToolExecutor),
            BrainServices {
                session_storage: Some(storage.clone()),
                ..BrainServices::default()
            },
            crate::provider::fake::unscripted_factory(),
        )
    };
    let crashed = compose("brain-storage-copy-owner-a");
    let created = crashed
        .create_session(
            typed_create_result(json!({
                "model": {
                    "provider": "anthropic",
                    "name": "storage-copy-crash-test",
                    "api_key": "sk-test"
                }
            }))
            .expect("valid copy crash create"),
            None,
        )
        .await
        .expect("create copy crash root");
    let session_id = created.id.to_string();
    let ticket = crashed
        .storage_prepare_upload(
            &session_id,
            crate::storage::StorageUploadIntent {
                key: "copied.bin".into(),
                bytes: 8,
                sha256: Some("c".repeat(64)),
                content_type: Some("application/octet-stream".into()),
                overwrite: false,
            },
        )
        .await
        .expect("reserve copy crash upload");

    // The adapter publishes destination bytes, then the Brain process loses the response
    // before it can journal StorageUploadPublished.
    storage
        .staged
        .lock()
        .expect("staged uploads")
        .insert(ticket.transfer_id.clone());
    crate::storage::SessionStoragePort::complete_upload(
        storage.as_ref(),
        &session_id,
        &ticket.transfer_id,
    )
    .await
    .expect("simulate successful CopyObject with lost response");
    let mut expired = journal
        .cloned_as("brain-storage-copy-owner-a")
        .claim(&session_id)
        .await
        .expect("claim copied reservation");
    expired
        .doc
        .storage_upload
        .as_mut()
        .expect("copied reservation")
        .expires_at_ms = crate::wall_ms().saturating_sub(1);
    let mut lease = Lease {
        fence: expired.fence,
        last_seq: expired.last_seq,
        retention: expired.retention,
    };
    let expirer = journal.cloned_as("brain-storage-copy-owner-a");
    expirer
        .commit(&session_id, &mut lease, &[], &expired.doc, expired.last_seq)
        .await
        .expect("persist post-crash expired reservation");
    expirer
        .release(&session_id, &lease)
        .await
        .expect("release crashed owner");

    let recovering = compose("brain-storage-copy-owner-b");
    recovering.start_recovery_worker();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let head = journal.get_head(&session_id).await.unwrap();
            if head
                .doc
                .storage_upload
                .as_ref()
                .is_some_and(|upload| upload.state == UploadReservationState::Completed)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("due worker adopts copied bytes without customer traffic");

    let adopted = journal.get_head(&session_id).await.expect("adopted head");
    assert_eq!(adopted.doc.session_storage_bytes, 8);
    assert_eq!(adopted.doc.storage_reserved_bytes, 0);
    assert_eq!(adopted.doc.tenant_metered_storage_bytes, 8);
    let records = journal.read_records(&session_id, 0).await.unwrap();
    assert!(
        records
            .iter()
            .any(|entry| matches!(entry.record, Record::StorageUploadPublished { .. }))
    );
    assert!(
        records
            .iter()
            .any(|entry| matches!(entry.record, Record::StorageUploadCompleted { .. }))
    );
    assert!(
        !records
            .iter()
            .any(|entry| matches!(entry.record, Record::StorageUploadExpired { .. }))
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn storage_upload_reservation_is_durable_bounded_and_retried_after_restart() {
    let data_dir = std::env::temp_dir().join(format!(
        "brain-storage-reservation-{}-{}",
        std::process::id(),
        crate::wall_ms()
    ));
    std::fs::create_dir_all(&data_dir).expect("create storage reservation data dir");
    let journal = Journal::new_memory("brain-storage-reservation");
    let storage = Arc::new(ReservationStorage::new(journal.clone()));
    let cfg = BrainConfig {
        idle_discard: Duration::from_secs(300),
        storage_max_object_bytes: 8,
        storage_max_session_bytes: 20,
        storage_transfer_ttl: Duration::from_secs(60 * 60),
        ..BrainConfig::default()
    };
    let compose = |storage: Arc<ReservationStorage>| {
        Brain::with_parts_and_services(
            cfg.clone(),
            journal.clone(),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(crate::adapter::DisabledToolExecutor),
            BrainServices {
                session_storage: Some(storage),
                customer_delivery: None,
                customer_transport: None,
                compactor: None,
                ..BrainServices::default()
            },
            crate::provider::fake::unscripted_factory(),
        )
    };
    let brain = compose(storage.clone());
    let created = brain
        .create_session(
            typed_create_result(json!({
                "model": {
                    "provider": "anthropic",
                    "name": "storage-test",
                    "api_key": "sk-test"
                }
            }))
            .expect("valid storage test create"),
            None,
        )
        .await
        .expect("create storage test session");
    let session_id = created.id.to_string();
    let too_large = brain
        .storage_prepare_upload(
            &session_id,
            crate::storage::StorageUploadIntent {
                key: "large.bin".into(),
                bytes: 9,
                sha256: Some("a".repeat(64)),
                content_type: None,
                overwrite: false,
            },
        )
        .await;
    assert!(matches!(
        too_large,
        Err(BrainError::StorageObjectTooLarge { limit: 8 })
    ));
    assert_eq!(storage.prepares.load(Ordering::Relaxed), 0);

    let ticket = brain
        .storage_prepare_upload(
            &session_id,
            crate::storage::StorageUploadIntent {
                key: "bounded.bin".into(),
                bytes: 8,
                sha256: Some("b".repeat(64)),
                content_type: Some("application/octet-stream".into()),
                overwrite: false,
            },
        )
        .await
        .expect("reserve bounded upload");
    assert!(storage.saw_durable_reservation.load(Ordering::Relaxed));
    let reserved = journal.get_head(&session_id).await.expect("reserved head");
    assert_eq!(reserved.doc.session_storage_bytes, 0);
    assert_eq!(reserved.doc.storage_reserved_bytes, 8);
    assert_eq!(
        reserved
            .doc
            .storage_upload
            .as_ref()
            .map(|upload| upload.transfer_id.as_str()),
        Some(ticket.transfer_id.as_str())
    );
    let competing = brain
        .storage_prepare_upload(
            &session_id,
            crate::storage::StorageUploadIntent {
                key: "other.bin".into(),
                bytes: 1,
                sha256: Some("c".repeat(64)),
                content_type: None,
                overwrite: false,
            },
        )
        .await;
    assert!(matches!(
        competing,
        Err(BrainError::StorageUploadInProgress { .. })
    ));

    // Simulate process loss after the reservation by advancing only its persisted deadline.
    let mut crashed = journal
        .claim(&session_id)
        .await
        .expect("claim expired upload");
    crashed
        .doc
        .storage_upload
        .as_mut()
        .expect("reservation")
        .expires_at_ms = crate::wall_ms().saturating_sub(1);
    let mut lease = Lease {
        fence: crashed.fence,
        last_seq: crashed.last_seq,
        retention: crashed.retention,
    };
    journal
        .commit(&session_id, &mut lease, &[], &crashed.doc, crashed.last_seq)
        .await
        .expect("persist expired deadline");
    journal
        .release(&session_id, &lease)
        .await
        .expect("release crashed storage owner");

    let restarted = compose(storage.clone());
    storage.fail_next_abort.store(true, Ordering::Relaxed);
    assert!(restarted.storage_reconcile(&session_id).await.is_err());
    let after_failure = journal
        .get_head(&session_id)
        .await
        .expect("head after abort failure");
    assert_eq!(after_failure.doc.storage_reserved_bytes, 8);
    assert!(after_failure.doc.storage_upload.is_some());
    restarted
        .storage_reconcile(&session_id)
        .await
        .expect("retry expired staging deletion");
    let after_retry = journal
        .get_head(&session_id)
        .await
        .expect("head after cleanup");
    assert_eq!(after_retry.doc.storage_reserved_bytes, 0);
    assert!(after_retry.doc.storage_upload.is_none());
    assert_eq!(storage.aborts.load(Ordering::Relaxed), 2);
    let storage_records = journal
        .read_records(&session_id, 0)
        .await
        .expect("storage journal");
    assert!(
        storage_records
            .iter()
            .any(|entry| matches!(entry.record, Record::StorageUploadExpired { .. }))
    );
    let gauges: Vec<_> = storage_records
        .iter()
        .filter_map(|entry| {
            crate::events::derive(&session_id, entry.seq, entry.ts_ms, &entry.record)
        })
        .filter_map(|event| match event {
            brain_protocol::session::Event::StorageUsage { storage, .. } => {
                Some((storage.session_storage_bytes, storage.upload_reserved_bytes))
            }
            _ => None,
        })
        .collect();
    assert_eq!(gauges, vec![(0, 8), (0, 0)]);
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn cancellation_during_managed_submit_is_durable_before_cleanup() {
    struct DispatchLoop {
        name: String,
    }

    #[async_trait::async_trait]
    impl crate::agentloop::Agentloop for DispatchLoop {
        async fn drive_turn(
            &self,
            ctx: &mut dyn crate::agentloop::TurnCtx,
        ) -> Result<crate::agentloop::LoopVerdict> {
            use brain_protocol::agentloop::{AgentloopErrorCode, CtxOp};
            let call = serde_json::from_value(json!({
                "tool_call_id": "call-managed-cancel",
                "name": self.name,
                "input": {"sleep": 30}
            }))?;
            match ctx
                .contract_op(CtxOp::ToolsDispatch { calls: vec![call] })
                .await?
            {
                Err(error) if error.code == AgentloopErrorCode::Aborted => {
                    Ok(crate::agentloop::LoopVerdict {
                        stop_reason: TurnStopReason::Cancelled,
                        terminal_committed: false,
                    })
                }
                outcome => Err(BrainError::Agentloop(format!(
                    "cancelled dispatch returned {outcome:?}"
                ))),
            }
        }
    }

    struct DispatchRegistry {
        name: String,
    }

    impl crate::agentloop::AgentloopRegistry for DispatchRegistry {
        fn resolve(
            &self,
            _selector: &crate::journal::AgentloopSelectorDoc,
        ) -> Result<Arc<dyn crate::agentloop::Agentloop>> {
            Ok(Arc::new(DispatchLoop {
                name: self.name.clone(),
            }))
        }

        fn admit_custom(
            &self,
            source_bundle_sha256: &str,
            toolchain: &str,
            bundle: &[u8],
        ) -> Result<crate::journal::AgentloopSelectorDoc> {
            Ok(crate::journal::AgentloopSelectorDoc {
                source_bundle_sha256: source_bundle_sha256.into(),
                source_bundle_bytes: bundle.len() as u64,
                toolchain: toolchain.into(),
            })
        }
    }

    let journal = Journal::new_memory("brain-managed-submit-live-cancel");
    let ports = Arc::new(UnknownManagedPorts::default());
    ports.block_submit.store(true, Ordering::Release);
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    let name = "managed_cancel_test".to_owned();
    let provider = fake.clone();
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            idle_discard: Duration::from_secs(300),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(crate::adapter::DisabledToolExecutor),
        BrainServices {
            agentloop_registry: Some(Arc::new(DispatchRegistry { name: name.clone() })),
            bundle_storage: Some(Arc::new(TestBundleStorage)),
            environments: test_environment_registry(
                "test.managed",
                ports.clone(),
                ports.clone(),
                Some(ports.clone()),
            ),
            ..BrainServices::default()
        },
        Arc::new(move |_| provider.clone()),
    );
    let created = brain
        .create_session(
            typed_create(json!({
                "model":{"provider":"anthropic","name":"managed-cancel","api_key":"key"}
            })),
            Some("managed-submit-live-cancel"),
        )
        .await
        .expect("create live cancellation session");
    let session_id = created.id.to_string();
    let mut resident = hydrate(&brain, &session_id)
        .await
        .expect("claim live cancellation session");
    let bundle_digest = "a".repeat(64);
    declare_test_managed_environment(&mut resident.st.head, &name, &bundle_digest);
    resident.st.head.prefix.environment_enabled = true;
    resident.st.head.prefix.tools = serde_json::from_value(json!([{
        "definition": {
            "name":name,
            "description":"block until cancellation",
            "contract_digest":"b".repeat(64),
            "input_schema":{"type":"object"},
            "output_schema":{}
        },
        "executor": {
            "kind":"environment",
            "environment":"workspace",
            "artifact_digest":bundle_digest,
            "requirements":{}
        }
    }]))
    .expect("managed Tool seal");
    let binding: brain_protocol::environment::ResolvedBinding = serde_json::from_value(json!({
        "binding_ref":"bnd_managedcancel0000",
        "capabilities":["execution","session_preparation"],
        "environment_id":"environment_managedcancel",
        "limits":{
            "max_inline_input_bytes":brain_protocol::MAX_MANAGED_TOOL_INPUT_BYTES,
            "max_inline_result_bytes":brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
            "max_wait_ms":1
        },
        "recovery":"retained"
    }))
    .expect("valid managed binding");
    resident.managed_bindings = Arc::new(HashMap::from([(
        name,
        crate::environment::ManagedBinding {
            environment_name: "workspace".into(),
            resolved: binding,
            environment: ports.clone(),
        },
    )]));

    let content = vec![ContentBlock::text("run the managed effect")];
    let (turn, user_seq, cancel) = admit(
        &brain,
        &session_id,
        &mut resident,
        content.clone(),
        HashMap::new(),
        None,
    )
    .await
    .expect("admit managed turn");
    let run = turn_run(
        &brain,
        &session_id,
        &turn,
        &resident,
        HashMap::new(),
        cancel.clone(),
        Some(crate::turn::AdmittedMessage {
            seq: user_seq,
            at_ms: crate::wall_ms(),
            content,
        }),
    )
    .expect("build managed turn");
    let mut state = resident.st;
    let running = tokio::spawn(async move {
        let outcome = run.run(&mut state).await;
        (state, outcome)
    });
    tokio::time::timeout(Duration::from_secs(5), ports.submit_started.notified())
        .await
        .expect("managed Submit began");
    cancel.cancel();

    let (state, report) = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("cancelled managed turn completed")
        .expect("managed turn task");
    let report = report.expect("cancelled turn report");
    assert_eq!(report.stop_reason, TurnStopReason::Cancelled);
    assert_eq!(ports.submits.load(Ordering::Acquire), 1);
    let records = journal.read_records(&session_id, 0).await.unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|entry| matches!(entry.record, Record::ManagedCallUnknown { .. }))
            .count(),
        1,
        "cancellation durably revokes every future Submit replay"
    );
    assert_eq!(
        records
            .iter()
            .filter(|entry| matches!(
                &entry.record,
                Record::TurnCompleted { stop_reason, .. }
                    if *stop_reason == TurnStopReason::Cancelled
            ))
            .count(),
        1
    );
    fake.assert_drained(0, "contract managed cancellation")
        .unwrap();
    journal
        .release(&session_id, &state.lease)
        .await
        .expect("release cancelled managed turn owner");
}

#[tokio::test]
async fn a_cancelled_submit_concludes_before_sandbox_reconciliation() {
    let journal = Journal::new_memory("brain-managed-submit-cancel-materializing");
    let ports = Arc::new(UnknownManagedPorts::default());
    ports.block_submit.store(true, Ordering::Release);
    // The live plane answers status with retryable materialization-in-progress while the
    // detached submit still drives the launch; a cancelled turn must conclude anyway.
    ports.status_materializing.store(true, Ordering::Release);
    let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
    let name = "managed_cancel_test".to_owned();
    fake.script([Scripted::tool(&name, json!({"sleep":30}))]);
    let provider = fake.clone();
    let brain = Brain::with_parts_and_services(
        BrainConfig {
            idle_discard: Duration::from_secs(300),
            ..BrainConfig::default()
        },
        journal.clone(),
        Arc::new(crate::keys::PlainCustody),
        Arc::new(crate::adapter::DisabledToolExecutor),
        BrainServices {
            bundle_storage: Some(Arc::new(TestBundleStorage)),
            environments: test_environment_registry(
                "test.managed",
                ports.clone(),
                ports.clone(),
                Some(ports.clone()),
            ),
            ..BrainServices::default()
        },
        Arc::new(move |_| provider.clone()),
    );
    let created = brain
        .create_session(
            typed_create(json!({
                "model":{"provider":"anthropic","name":"managed-cancel","api_key":"key"}
            })),
            Some("managed-submit-cancel-mat"),
        )
        .await
        .expect("create live cancellation session");
    let session_id = created.id.to_string();
    let mut resident = hydrate(&brain, &session_id)
        .await
        .expect("claim live cancellation session");
    let bundle_digest = "a".repeat(64);
    declare_test_managed_environment(&mut resident.st.head, &name, &bundle_digest);
    resident.st.head.prefix.environment_enabled = true;
    resident.st.head.prefix.tools = serde_json::from_value(json!([{
        "definition": {
            "name":name,
            "description":"block until cancellation",
            "contract_digest":"b".repeat(64),
            "input_schema":{"type":"object"},
            "output_schema":{}
        },
        "executor": {
            "kind":"environment",
            "environment":"workspace",
            "artifact_digest":bundle_digest,
            "requirements":{}
        }
    }]))
    .expect("managed Tool seal");
    let binding: brain_protocol::environment::ResolvedBinding = serde_json::from_value(json!({
        "binding_ref":"bnd_managedcancel0000",
        "capabilities":["execution","session_preparation"],
        "environment_id":"environment_managedcancel",
        "limits":{
            "max_inline_input_bytes":brain_protocol::MAX_MANAGED_TOOL_INPUT_BYTES,
            "max_inline_result_bytes":brain_protocol::MAX_TOOL_TERMINAL_INLINE_BYTES,
            "max_wait_ms":1
        },
        "recovery":"retained"
    }))
    .expect("valid managed binding");
    resident.managed_bindings = Arc::new(HashMap::from([(
        name,
        crate::environment::ManagedBinding {
            environment_name: "workspace".into(),
            resolved: binding,
            environment: ports.clone(),
        },
    )]));

    let content = vec![ContentBlock::text("run the managed effect")];
    let (turn, user_seq, cancel) = admit(
        &brain,
        &session_id,
        &mut resident,
        content.clone(),
        HashMap::new(),
        None,
    )
    .await
    .expect("admit managed turn");
    let run = turn_run(
        &brain,
        &session_id,
        &turn,
        &resident,
        HashMap::new(),
        cancel.clone(),
        Some(crate::turn::AdmittedMessage {
            seq: user_seq,
            at_ms: crate::wall_ms(),
            content,
        }),
    )
    .expect("build managed turn");
    let mut state = resident.st;
    let running = tokio::spawn(async move {
        let outcome = run.run(&mut state).await;
        (state, outcome)
    });
    tokio::time::timeout(Duration::from_secs(5), ports.submit_started.notified())
        .await
        .expect("managed Submit began");
    cancel.cancel();

    let (state, report) = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("cancelled managed turn completed")
        .expect("managed turn task");
    let report = report.expect("cancelled turn report");
    assert_eq!(report.stop_reason, TurnStopReason::Cancelled);
    assert_eq!(ports.submits.load(Ordering::Acquire), 1);
    let records = journal.read_records(&session_id, 0).await.unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|entry| matches!(entry.record, Record::ManagedCallUnknown { .. }))
            .count(),
        1,
        "cancellation durably revokes every future Submit replay"
    );
    assert_eq!(
        records
            .iter()
            .filter(|entry| matches!(
                &entry.record,
                Record::TurnCompleted { stop_reason, .. }
                    if *stop_reason == TurnStopReason::Cancelled
            ))
            .count(),
        1
    );
    fake.assert_drained(1, "live managed cancellation").unwrap();
    journal
        .release(&session_id, &state.lease)
        .await
        .expect("release cancelled managed turn owner");
}
