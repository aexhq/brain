use super::*;

fn user(turn: &str, text: &str) -> Record {
    Record::UserMessage {
        turn: turn.into(),
        content: vec![ContentBlock::text(text)],
        starts_turn: false,
        metadata: HashMap::new(),
        idempotency_key_hash: None,
        request_hash: None,
    }
}
fn assistant(turn: &str, blocks: Vec<ContentBlock>) -> Record {
    Record::Assistant {
        turn: turn.into(),
        agent: "root".into(),
        attempt_id: "att_aaaaaaaaaaaaaaaaaaaa".into(),
        content: blocks,
        stop: StopReason::EndTurn,
    }
}
fn result(call: &str, content: &str, is_error: bool) -> Record {
    Record::ToolResult {
        turn: "t1".into(),
        agent: "root".into(),
        call: call.into(),
        name: "bash".into(),
        outcome: if is_error { "failed" } else { "completed" }.into(),
        content: content.into(),
        is_error,
        exit_code: Some(if is_error { 1 } else { 0 }),
        duration_ms: 5,
        truncated: false,
    }
}
fn entries(records: Vec<Record>) -> Vec<Entry> {
    records
        .into_iter()
        .enumerate()
        .map(|(i, record)| Entry {
            seq: i as u64 + 1,
            ts_ms: 0,
            record,
        })
        .collect()
}

async fn create_memory_store(
    store: &MemoryStore,
    session_id: &str,
    doc: &HeadDoc,
    first: &Record,
    owner: &str,
    now_ms: u64,
) -> Result<()> {
    let limits = JournalRetentionLimits::default();
    let retention = initial_retention(first, limits.session_bytes)?;
    store
        .create(
            session_id,
            doc,
            first,
            owner,
            now_ms,
            u64::MAX,
            retention,
            limits,
        )
        .await
}

#[test]
fn pre_idempotency_user_records_remain_readable() {
    let record: Record = serde_json::from_value(serde_json::json!({
        "kind": "user_message",
        "turn": "trn_old",
        "content": [{"type": "text", "text": "hello"}],
        "metadata": {}
    }))
    .unwrap();
    let Record::UserMessage {
        idempotency_key_hash,
        request_hash,
        ..
    } = record
    else {
        panic!("expected user message");
    };
    assert!(idempotency_key_hash.is_none());
    assert!(request_hash.is_none());
}

#[test]
fn fold_rebuilds_the_conversation_and_groups_consecutive_tool_results() {
    let f = fold(&entries(vec![
        user("t1", "build it"),
        Record::TurnStarted { turn: "t1".into() },
        assistant(
            "t1",
            vec![
                ContentBlock::text("running"),
                ContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "bash".into(),
                    input: serde_json::json!({"command":"a"}),
                },
                ContentBlock::ToolUse {
                    id: "c2".into(),
                    name: "bash".into(),
                    input: serde_json::json!({"command":"b"}),
                },
            ],
        ),
        result("c1", "ok-a", false),
        result("c2", "boom", true),
        assistant("t1", vec![ContentBlock::text("done")]),
    ]));
    assert_eq!(
        f.history.len(),
        4,
        "user, assistant, ONE grouped results message, assistant"
    );
    assert_eq!(f.history[2].role, Role::User);
    assert_eq!(f.history[2].content.len(), 2, "both results in one message");
    assert!(matches!(
        &f.history[2].content[1],
        ContentBlock::ToolResult { is_error: true, .. }
    ));
    assert_eq!(f.turns, 1);
}

#[test]
fn fold_flushes_trailing_results_at_finish() {
    // A crash after committing results but before the next assistant message must still
    // rebuild a history the provider will accept.
    let f = fold(&entries(vec![
        user("t1", "x"),
        assistant(
            "t1",
            vec![ContentBlock::ToolUse {
                id: "c1".into(),
                name: "bash".into(),
                input: serde_json::json!({}),
            }],
        ),
        result("c1", "out", false),
    ]));
    assert_eq!(f.history.len(), 3);
    assert_eq!(f.history[2].role, Role::User);
}

#[test]
fn subagent_records_never_split_or_pollute_root_history() {
    let mut child_assistant = assistant("t1", vec![ContentBlock::text("child")]);
    if let Record::Assistant { agent, .. } = &mut child_assistant {
        *agent = "agt_child".into();
    }
    let mut child_result = result("child-call", "child-out", false);
    if let Record::ToolResult { agent, .. } = &mut child_result {
        *agent = "agt_child".into();
    }
    let f = fold(&entries(vec![
        user("t1", "go"),
        assistant(
            "t1",
            vec![
                ContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "task".into(),
                    input: serde_json::json!({}),
                },
                ContentBlock::ToolUse {
                    id: "c2".into(),
                    name: "task".into(),
                    input: serde_json::json!({}),
                },
            ],
        ),
        result("c1", "one", false),
        child_assistant,
        child_result,
        result("c2", "two", false),
        assistant("t1", vec![ContentBlock::text("done")]),
    ]));
    assert_eq!(f.history.len(), 4);
    assert_eq!(f.history[2].content.len(), 2);
    assert!(f.history.iter().all(|message| {
        message
            .content
            .iter()
            .all(|block| !matches!(block, ContentBlock::Text { text } if text == "child"))
    }));
}

#[test]
fn next_user_text_merges_with_an_interrupted_tool_result() {
    let f = fold(&entries(vec![
        user("t1", "start"),
        assistant(
            "t1",
            vec![ContentBlock::ToolUse {
                id: "c1".into(),
                name: "task".into(),
                input: serde_json::json!({}),
            }],
        ),
        result("c1", "subagent interrupted", true),
        Record::TurnCompleted {
            turn: "t1".into(),
            stop_reason: "interrupted".into(),
            rounds: 1,
            tool_calls: 1,
            result: None,
        },
        user("t2", "continue"),
    ]));
    assert_eq!(f.history.len(), 3);
    assert_eq!(f.history[2].role, Role::User);
    assert!(matches!(
        &f.history[2].content[..],
        [ContentBlock::ToolResult { is_error: true, .. }, ContentBlock::Text { text }]
            if text == "continue"
    ));
}

#[test]
fn fold_is_a_loop_over_apply() {
    // F1 (donor property): batch fold == incremental apply, at every prefix.
    let all = entries(vec![
        user("t1", "a"),
        assistant("t1", vec![ContentBlock::text("b")]),
        user("t2", "c"),
        result("c9", "r", false),
        assistant("t2", vec![ContentBlock::text("d")]),
    ]);
    for split in 0..=all.len() {
        let mut inc = Fold::default();
        for e in &all[..split] {
            inc.apply(&e.record);
        }
        inc.finish();
        let batch = fold(&all[..split]);
        assert_eq!(batch.history, inc.history, "split {split}");
    }
}

#[test]
fn checkpoint_records_do_not_mutate_the_raw_audit_fold() {
    let f = fold(&entries(vec![
        user("t1", "one"),
        assistant("t1", vec![ContentBlock::text("1")]),
        user("t2", "two"),
        assistant("t2", vec![ContentBlock::text("2")]),
        Record::ContextInstalled {
            checkpoint_id: "ctx_1".into(),
            base_checkpoint_id: None,
            covers_through_sequence: 4,
            retained_messages: 2,
            payload_digest: "a".repeat(64),
            base_prefix_digest: "b".repeat(64),
            source_context_digest: "c".repeat(64),
            token_estimate: 4,
            context_generation: 1,
            summary_kind: "semantic".into(),
            compactor_provider: "fake".into(),
            compactor_model: "fake".into(),
            retained_from_sequence: 1,
            created_at_ms: 0,
        },
        user("t3", "three"),
    ]));
    assert_eq!(
        f.history.len(),
        5,
        "raw fold remains an audit reconstruction"
    );
    assert_eq!(f.history[0], Message::user_text("one"));
}

#[test]
fn unknown_record_kind_is_a_typed_error_not_a_passthrough() {
    let bad = r#"{"kind":"totally_new","x":1}"#;
    assert!(serde_json::from_str::<Record>(bad).is_err());
}

#[test]
fn record_sks_sort_numerically() {
    assert!(record_sk(9) < record_sk(10));
    assert!(record_sk(999) < record_sk(1000));
}

#[test]
fn head_doc_round_trips() {
    let doc = HeadDoc {
        loop_state: None,
        tenant_id: "local".into(),
        root_id: "ses_test".into(),
        parent_id: None,
        ancestor_ids: Vec::new(),
        child_name: None,
        context_fork: None,
        depth: 0,
        last_seq: 1,
        state: "open".into(),
        failure: None,
        turn: None,
        active_phase: None,
        provider_attempt: None,
        active_context: HashMap::new(),
        active_rounds: 0,
        active_tool_calls: 0,
        message_replays: vec![],
        context: None,
        turns: 0,
        created_ms: 1,
        updated_ms: 2,
        recovery_due_ms: None,
        recovery_attempt: 0,
        create_key_hash: None,
        create_request_hash: None,
        last_message_ms: None,
        ended: false,
        prefix: PrefixDoc {
            agentloop: None,
            system_prompt: Some("sp".into()),
            provider: "anthropic".into(),
            model: "claude".into(),
            base_url: None,
            max_output_tokens: Some(4096),
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
            max_additional_sandboxes_per_root: 2,
            network: serde_json::json!({"outbound":"none"}),
            customer_client_id: None,
            customer_submit_retries: 1,
            rendered_base: serde_json::json!({}),
            rendered_base_digest: String::new(),
            prompt_cache_key: String::new(),
            tools: vec![],
            managed_bundles: vec![],
            official_capabilities: HashMap::new(),
            hand_enabled: true,
            shape: "1gb".into(),
            sync_interval_seconds: 600,
            hand_env_keys: vec![],
            metadata: HashMap::new(),
        },
        key_b64: "AAAA".into(),
        hand_secrets_b64: String::new(),
        session_storage_bytes: 0,
        storage_reserved_bytes: 0,
        tenant_metered_storage_bytes: 0,
        storage_upload: None,
        storage_delete: None,
        pending_customer_acks: vec![],
        pending_managed_acks: vec![],
        default_sandbox: None,
    };
    let s = serde_json::to_string(&doc).unwrap();
    let back: HeadDoc = serde_json::from_str(&s).unwrap();
    assert_eq!(back.prefix.model, "claude");
    assert_eq!(back.state, "open");
}

fn head_doc() -> HeadDoc {
    HeadDoc {
        loop_state: None,
        tenant_id: "local".into(),
        root_id: "ses_test".into(),
        parent_id: None,
        ancestor_ids: Vec::new(),
        child_name: None,
        context_fork: None,
        depth: 0,
        last_seq: 1,
        state: "open".into(),
        failure: None,
        turn: None,
        active_phase: None,
        provider_attempt: None,
        active_context: HashMap::new(),
        active_rounds: 0,
        active_tool_calls: 0,
        message_replays: vec![],
        context: None,
        turns: 0,
        created_ms: 1,
        updated_ms: 1,
        recovery_due_ms: None,
        recovery_attempt: 0,
        create_key_hash: None,
        create_request_hash: None,
        last_message_ms: None,
        ended: false,
        prefix: PrefixDoc {
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
            max_additional_sandboxes_per_root: 2,
            network: serde_json::json!({"outbound":"none"}),
            customer_client_id: None,
            customer_submit_retries: 1,
            rendered_base: serde_json::json!({}),
            rendered_base_digest: String::new(),
            prompt_cache_key: String::new(),
            tools: vec![],
            managed_bundles: vec![],
            official_capabilities: HashMap::new(),
            hand_enabled: false,
            shape: "1gb".into(),
            sync_interval_seconds: 600,
            hand_env_keys: vec![],
            metadata: HashMap::new(),
        },
        key_b64: String::new(),
        hand_secrets_b64: String::new(),
        session_storage_bytes: 0,
        storage_reserved_bytes: 0,
        tenant_metered_storage_bytes: 0,
        storage_upload: None,
        storage_delete: None,
        pending_customer_acks: vec![],
        pending_managed_acks: vec![],
        default_sandbox: None,
    }
}

#[test]
fn decision_limits_reject_oversized_items_and_aggregate_batches_before_store_io() {
    let doc = head_doc();
    let oversized = Record::Assistant {
        turn: "turn".into(),
        agent: "root".into(),
        attempt_id: "att_aaaaaaaaaaaaaaaaaaaa".into(),
        content: vec![ContentBlock::text("x".repeat(MAX_SERIALIZED_RECORD_BYTES))],
        stop: StopReason::EndTurn,
    };
    let error = validate_decision("ses_limits", &[(2, oversized)], &doc).unwrap_err();
    assert!(error.to_string().contains("assistant record"));

    let near_max_results = (0..40)
        .map(|index| {
            (
                index + 2,
                Record::ToolResult {
                    turn: "turn".into(),
                    agent: "root".into(),
                    call: format!("call_{index}"),
                    name: "tool".into(),
                    outcome: "completed".into(),
                    content: "x".repeat(MAX_RECORD_CONTENT_BYTES),
                    is_error: false,
                    exit_code: None,
                    duration_ms: 1,
                    truncated: false,
                },
            )
        })
        .collect::<Vec<_>>();
    let error = validate_decision("ses_limits", &near_max_results, &doc).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("journal decision is approximately")
    );

    let too_many = (0..MAX_DECISION_ACTIONS)
        .map(|index| {
            (
                index as u64 + 2,
                Record::State {
                    state: "open".into(),
                    turn: None,
                },
            )
        })
        .collect::<Vec<_>>();
    let error = validate_decision("ses_limits", &too_many, &doc).unwrap_err();
    assert!(error.to_string().contains("actions"));

    let mut oversized_listing = doc;
    oversized_listing
        .prefix
        .metadata
        .insert("large".into(), "x".repeat(MAX_SERIALIZED_LISTING_BYTES));
    let error = validate_decision("ses_limits", &[], &oversized_listing).unwrap_err();
    assert!(error.to_string().contains("listing document"));
}

#[tokio::test]
async fn memory_journal_full_lifecycle() {
    let j = Journal::new_memory("brain-a");
    let doc = head_doc();
    j.create(
        "ses_m",
        &doc,
        &Record::State {
            state: "open".into(),
            turn: None,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        j.create(
            "ses_m",
            &doc,
            &Record::State {
                state: "open".into(),
                turn: None
            }
        )
        .await,
        Err(BrainError::Invalid(_))
    ));

    let head = j.claim("ses_m").await.unwrap();
    assert_eq!(
        head.fence, 1,
        "create is unowned; the first claim establishes fence 1"
    );
    assert_eq!(head.last_seq, 1);

    let mut lease = Lease {
        fence: head.fence,
        last_seq: head.last_seq,
        retention: head.retention,
    };
    let rec = (2u64, Record::TurnStarted { turn: "t1".into() });
    j.commit("ses_m", &mut lease, std::slice::from_ref(&rec), &doc, 3)
        .await
        .unwrap();
    assert_eq!(
        lease.last_seq, 3,
        "high water persisted, ephemeral seq included"
    );

    let entries = j.read_records("ses_m", 0).await.unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].seq, 2);

    // Re-committing the same seq is a superseded write, exactly like DynamoDB.
    assert!(matches!(
        j.commit("ses_m", &mut lease, std::slice::from_ref(&rec), &doc, 4)
            .await,
        Err(BrainError::Fenced)
    ));

    let deletion_head = j.get_head("ses_m").await.unwrap();
    assert_eq!(j.purge_history("ses_m").await.unwrap(), 2);
    j.finalize_deletion(&DeletionStatusDoc {
        session_id: "ses_m".into(),
        tenant_id: deletion_head.doc.tenant_id.clone(),
        root_id: deletion_head.doc.root_id.clone(),
        parent_id: deletion_head.doc.parent_id.clone(),
        metered_storage_bytes: deletion_head.doc.tenant_metered_storage_bytes,
        metered_journal_bytes: deletion_head.retention.metered_bytes,
        state: "succeeded".into(),
        requested_at_ms: 1,
        updated_at_ms: 2,
        completed_at_ms: Some(2),
        expires_at_ms: i64::MAX as u64,
        attempts: 1,
        last_error: None,
    })
    .await
    .unwrap();
    assert!(matches!(
        j.get_head("ses_m").await,
        Err(BrainError::NoSuchSession(_))
    ));
}

#[tokio::test]
async fn record_pages_are_bounded_and_stop_at_a_fixed_high_water() {
    let journal = Journal::new_memory("brain-page");
    let doc = head_doc();
    journal
        .create(
            "ses_page",
            &doc,
            &Record::State {
                state: "open".into(),
                turn: None,
            },
        )
        .await
        .unwrap();
    let head = journal.claim("ses_page").await.unwrap();
    let mut lease = Lease {
        fence: head.fence,
        last_seq: head.last_seq,
        retention: head.retention,
    };
    let records = (2..=7)
        .map(|seq| {
            (
                seq,
                Record::State {
                    state: "open".into(),
                    turn: None,
                },
            )
        })
        .collect::<Vec<_>>();
    // 8 and 9 model live-only provisional sequence gaps.
    journal
        .commit("ses_page", &mut lease, &records, &doc, 9)
        .await
        .unwrap();

    let mut cursor = 0;
    let mut seen = Vec::new();
    loop {
        let page = journal
            .read_record_page(&RecordPageQuery {
                session_id: "ses_page",
                after: cursor,
                through_seq: 9,
                limit: 2,
                max_bytes: DEFAULT_RECORD_PAGE_BYTES,
            })
            .await
            .unwrap();
        assert!(page.entries.len() <= 2);
        seen.extend(page.entries.iter().map(|entry| entry.seq));
        let Some(next) = page.next_after else {
            break;
        };
        cursor = next;
    }
    assert_eq!(seen, (1..=7).collect::<Vec<_>>());

    journal
        .commit(
            "ses_page",
            &mut lease,
            &[(10, Record::TurnStarted { turn: "t2".into() })],
            &doc,
            10,
        )
        .await
        .unwrap();
    let fixed = journal
        .read_record_page(&RecordPageQuery {
            session_id: "ses_page",
            after: 7,
            through_seq: 9,
            limit: 2,
            max_bytes: DEFAULT_RECORD_PAGE_BYTES,
        })
        .await
        .unwrap();
    assert!(fixed.entries.is_empty());
    assert!(fixed.next_after.is_none());
}

#[tokio::test]
async fn memory_journal_fences_out_a_stale_owner() {
    let a = Journal::new_memory("brain-a");
    let b = a.cloned_as("brain-b");
    let doc = head_doc();
    a.create(
        "ses_f",
        &doc,
        &Record::State {
            state: "open".into(),
            turn: None,
        },
    )
    .await
    .unwrap();
    let head_a = a.claim("ses_f").await.unwrap();
    let mut lease_a = Lease {
        fence: head_a.fence,
        last_seq: head_a.last_seq,
        retention: head_a.retention,
    };

    // B cannot steal while A's lease is live...
    assert!(matches!(b.claim("ses_f").await, Err(BrainError::Fenced)));

    // ...but after A releases, B claims with a HIGHER fence, and A's writes are dead.
    a.release("ses_f", &lease_a).await.unwrap();
    let head_b = b.claim("ses_f").await.unwrap();
    assert!(head_b.fence > head_a.fence);
    let rec = (2u64, Record::TurnStarted { turn: "t".into() });
    assert!(matches!(
        a.commit("ses_f", &mut lease_a, std::slice::from_ref(&rec), &doc, 2)
            .await,
        Err(BrainError::Fenced)
    ));
    let mut lease_b = Lease {
        fence: head_b.fence,
        last_seq: head_b.last_seq,
        retention: head_b.retention,
    };
    b.commit("ses_f", &mut lease_b, std::slice::from_ref(&rec), &doc, 2)
        .await
        .unwrap();
}

fn sandbox_reservation(index: usize) -> SandboxReserveRequest {
    let root_id = "ses_sandbox_root".to_string();
    let sandbox_id = format!("sbx_{index:02}");
    SandboxReserveRequest {
        root_id: root_id.clone(),
        owner_session_id: root_id.clone(),
        sandbox_id: sandbox_id.clone(),
        operation_id: format!("op_{index:02}"),
        request_digest: format!("{index:064x}"),
        generation_intent: format!("gen_{index:02}"),
        initial_status: serde_json::from_value(serde_json::json!({
            "target": {
                "kind": "additional",
                "session_id": root_id,
                "root_id": "ses_sandbox_root",
                "binding_ref": format!("bnd_{index:02}"),
                "sandbox_id": sandbox_id,
            },
            "state": "creating",
            "expires_at_ms": null,
        }))
        .unwrap(),
        now_ms: index as u64 + 1,
    }
}

#[tokio::test]
async fn sandbox_inventory_reserves_cap_atomically_and_keeps_terminal_tombstones() {
    let store = Arc::new(MemoryStore::default());
    let mut root = head_doc();
    root.root_id = "ses_sandbox_root".into();
    root.prefix.max_additional_sandboxes_per_root = 2;
    create_memory_store(
        &store,
        "ses_sandbox_root",
        &root,
        &Record::State {
            state: "open".into(),
            turn: None,
        },
        "owner",
        0,
    )
    .await
    .unwrap();

    let mut tasks = Vec::new();
    for index in 0..8 {
        let store = store.clone();
        tasks.push(tokio::spawn(async move {
            store.reserve_sandbox(&sandbox_reservation(index)).await
        }));
    }
    let mut created = Vec::new();
    let mut exhausted = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(item) => created.push(item),
            Err(BrainError::SandboxResourceExhausted) => exhausted += 1,
            Err(error) => panic!("unexpected reservation result: {error}"),
        }
    }
    assert_eq!(created.len(), 2);
    assert_eq!(exhausted, 6);

    let replay_request = sandbox_reservation(
        created[0]
            .sandbox_id
            .strip_prefix("sbx_")
            .unwrap()
            .parse()
            .unwrap(),
    );
    let replay = store.reserve_sandbox(&replay_request).await.unwrap();
    assert_eq!(replay.sandbox_id, created[0].sandbox_id);

    let mut terminal: brain_protocol::hand::SandboxStatus =
        serde_json::from_value(serde_json::to_value(&created[0].status).unwrap()).unwrap();
    terminal.state = brain_protocol::hand::SandboxState::Terminated;
    let tombstone = store
        .update_sandbox(&SandboxUpdateRequest {
            root_id: created[0].root_id.clone(),
            sandbox_id: created[0].sandbox_id.clone(),
            expected_version: created[0].version,
            status: terminal,
            release_slot: true,
            now_ms: 100,
        })
        .await
        .unwrap();
    assert!(tombstone.slot_released);
    assert_eq!(
        store
            .get_sandbox(&tombstone.root_id, &tombstone.sandbox_id)
            .await
            .unwrap()
            .status
            .state,
        brain_protocol::hand::SandboxState::Terminated
    );
    store
        .reserve_sandbox(&sandbox_reservation(9))
        .await
        .expect("confirmed termination releases exactly one slot");
    assert!(matches!(
        store
            .update_sandbox(&SandboxUpdateRequest {
                root_id: tombstone.root_id.clone(),
                sandbox_id: tombstone.sandbox_id.clone(),
                expected_version: tombstone.version,
                status: sandbox_reservation(0).initial_status,
                release_slot: false,
                now_ms: 101,
            })
            .await,
        Err(BrainError::SandboxGone)
    ));
}

#[tokio::test]
async fn lease_heartbeat_prevents_recovery_steal_until_it_stops() {
    let store = MemoryStore::default();
    let mut doc = head_doc();
    doc.state = "open".into();
    doc.turn = Some("trn_heartbeat".into());
    doc.active_phase = Some("model_running".into());
    create_memory_store(
        &store,
        "ses_heartbeat",
        &doc,
        &Record::State {
            state: "open".into(),
            turn: doc.turn.clone(),
        },
        "owner-a",
        0,
    )
    .await
    .unwrap();
    let claimed = store
        .claim("ses_heartbeat", "owner-a", 1)
        .await
        .expect("first owner claims the unowned create");
    assert_eq!(claimed.fence, 1);
    store
        .renew("ses_heartbeat", "owner-a", 1, 50_000, Some(115_000))
        .await
        .unwrap();
    assert!(matches!(
        store.claim("ses_heartbeat", "owner-b", 70_000).await,
        Err(BrainError::Fenced)
    ));
    let recovered = store
        .claim("ses_heartbeat", "owner-b", 116_000)
        .await
        .expect("stopped heartbeat becomes stealable after lease plus grace");
    assert_eq!(recovered.fence, 2);
}

#[tokio::test]
async fn lease_renewal_preserves_scheduled_upload_expiry_and_idle_due_absence() {
    let store = MemoryStore::default();
    let mut reserved = head_doc();
    reserved.storage_upload = Some(StorageUploadReservationDoc {
        transfer_id: "xfer_fixed".into(),
        key: "large.bin".into(),
        bytes: 10,
        sha256: Some("00".repeat(32)),
        content_type: None,
        overwrite: false,
        previous_bytes: 0,
        expires_at_ms: 900_000,
        state: "reserved".into(),
    });
    // Create itself must not smuggle a pre-existing public storage reservation. The test is
    // only exercising the independently durable expiry anchor carried by the reservation.
    reserved.storage_reserved_bytes = 0;
    reserved.recovery_due_ms = Some(900_000);
    create_memory_store(
        &store,
        "ses_reserved",
        &reserved,
        &Record::State {
            state: "open".into(),
            turn: None,
        },
        "owner-a",
        0,
    )
    .await
    .unwrap();
    store
        .claim("ses_reserved", "owner-a", 1)
        .await
        .expect("first owner claims the unowned create");
    store
        .renew("ses_reserved", "owner-a", 1, 100_000, None)
        .await
        .unwrap();
    assert_eq!(
        store
            .get_head("ses_reserved")
            .await
            .unwrap()
            .doc
            .recovery_due_ms,
        Some(900_000),
        "lease-only heartbeat must not postpone or replace the fixed upload expiry"
    );

    let idle = head_doc().with_recovery_projection(100_000);
    create_memory_store(
        &store,
        "ses_idle",
        &idle,
        &Record::State {
            state: "open".into(),
            turn: None,
        },
        "owner-a",
        0,
    )
    .await
    .unwrap();
    store
        .claim("ses_idle", "owner-a", 1)
        .await
        .expect("first owner claims the unowned create");
    store
        .renew("ses_idle", "owner-a", 1, 100_000, None)
        .await
        .unwrap();
    assert_eq!(
        store
            .get_head("ses_idle")
            .await
            .unwrap()
            .doc
            .recovery_due_ms,
        None,
        "quiescent lease renewal must not create recovery work"
    );
}

#[test]
fn unacknowledged_customer_terminal_keeps_a_quiescent_session_recoverable() {
    let mut doc = head_doc();
    doc.pending_customer_acks.push(CustomerTerminalAckDoc {
        turn: "trn_customer".into(),
        call: "op_customer".into(),
        client_id: "app".into(),
        process_id: "process:test".into(),
        request_digest: "a".repeat(64),
        terminal_digest: "b".repeat(64),
    });
    let projected = doc.with_recovery_projection(100_000);
    assert_eq!(
        projected.recovery_due_ms,
        Some(100_000 + LEASE_MS + STEAL_GRACE_MS)
    );
    let mut acknowledged = projected;
    acknowledged.pending_customer_acks.clear();
    assert_eq!(
        acknowledged
            .with_recovery_projection(200_000)
            .recovery_due_ms,
        None
    );
}

#[test]
fn accepted_end_remains_due_until_the_subtree_reaches_ended() {
    let mut doc = head_doc();
    doc.state = "ending".into();
    doc.ended = true;
    let projected = doc.with_recovery_projection(100_000);
    assert_eq!(
        projected.recovery_due_ms,
        Some(100_000 + LEASE_MS + STEAL_GRACE_MS)
    );

    let mut ended = projected;
    ended.state = "ended".into();
    assert_eq!(
        ended.with_recovery_projection(200_000).recovery_due_ms,
        None,
        "a fully converged end must leave no recovery anchor"
    );
}

#[tokio::test]
async fn successful_quiescent_commit_returns_the_canonical_cleared_projection() {
    let store = Arc::new(MemoryStore::default());
    let journal = Journal::new(store, "owner-a");
    let mut active = head_doc();
    active.state = "open".into();
    active.turn = Some("trn_done".into());
    active.active_phase = Some("model_running".into());
    journal
        .create(
            "ses_projection",
            &active,
            &Record::State {
                state: "open".into(),
                turn: active.turn.clone(),
            },
        )
        .await
        .unwrap();
    let head = journal.claim("ses_projection").await.unwrap();
    let mut doc = head.doc;
    doc.state = "open".into();
    doc.turn = None;
    doc.active_phase = None;
    let mut lease = Lease {
        fence: head.fence,
        last_seq: head.last_seq,
        retention: head.retention,
    };
    let persisted = journal
        .commit("ses_projection", &mut lease, &[], &doc, head.last_seq)
        .await
        .unwrap();
    assert_eq!(persisted.recovery_due_ms, None);
    journal
        .renew("ses_projection", &lease, false)
        .await
        .unwrap();
    assert_eq!(
        journal
            .get_head("ses_projection")
            .await
            .unwrap()
            .doc
            .recovery_due_ms,
        None,
        "a later heartbeat cannot resurrect the completed recovery row"
    );
}

#[tokio::test]
async fn final_deletion_atomically_replaces_content_anchor_with_bounded_tombstone() {
    let store = MemoryStore::default();
    let doc = head_doc();
    create_memory_store(
        &store,
        "ses_deleted",
        &doc,
        &Record::State {
            state: "deleting".into(),
            turn: None,
        },
        "owner-a",
        0,
    )
    .await
    .unwrap();
    let terminal = DeletionStatusDoc {
        session_id: "ses_deleted".into(),
        tenant_id: doc.tenant_id.clone(),
        root_id: "ses_deleted".into(),
        parent_id: None,
        metered_storage_bytes: 0,
        metered_journal_bytes: store
            .get_head("ses_deleted")
            .await
            .unwrap()
            .retention
            .metered_bytes,
        state: "succeeded".into(),
        requested_at_ms: 1,
        updated_at_ms: 2,
        completed_at_ms: Some(2),
        expires_at_ms: u64::MAX,
        attempts: 1,
        last_error: None,
    };
    store.finalize_deletion(&terminal).await.unwrap();
    assert!(matches!(
        store.get_head("ses_deleted").await,
        Err(BrainError::NoSuchSession(_))
    ));
    assert_eq!(
        store
            .get_deletion_status("ses_deleted")
            .await
            .unwrap()
            .unwrap(),
        terminal
    );
    let mut stale = terminal.clone();
    stale.state = "retrying".into();
    stale.completed_at_ms = None;
    store.put_deletion_status(&stale).await.unwrap();
    assert_eq!(
        store
            .get_deletion_status("ses_deleted")
            .await
            .unwrap()
            .unwrap()
            .state,
        "succeeded"
    );
}

#[tokio::test]
async fn tenant_storage_meter_is_shared_atomic_and_released_once() {
    let store = Arc::new(MemoryStore::default());
    let left_journal = Journal::new(store.clone(), "owner-a").with_tenant_storage_limit(10);
    let right_journal = left_journal.cloned_as("owner-b");
    let mut left = head_doc();
    left.tenant_id = "tenant-meter".into();
    left.root_id = "ses_meter_left".into();
    let mut right = head_doc();
    right.tenant_id = "tenant-meter".into();
    right.root_id = "ses_meter_right".into();
    for (journal, id, doc) in [
        (&left_journal, "ses_meter_left", &left),
        (&right_journal, "ses_meter_right", &right),
    ] {
        journal
            .create(
                id,
                doc,
                &Record::State {
                    state: "open".into(),
                    turn: None,
                },
            )
            .await
            .unwrap();
    }

    let left_head = left_journal.claim("ses_meter_left").await.unwrap();
    let mut left_lease = Lease {
        fence: left_head.fence,
        last_seq: left_head.last_seq,
        retention: left_head.retention,
    };
    let mut left_doc = left_head.doc;
    left_doc.storage_reserved_bytes = 6;
    left_doc = left_journal
        .commit(
            "ses_meter_left",
            &mut left_lease,
            &[],
            &left_doc,
            left_head.last_seq,
        )
        .await
        .unwrap();

    let right_head = right_journal.claim("ses_meter_right").await.unwrap();
    let mut right_lease = Lease {
        fence: right_head.fence,
        last_seq: right_head.last_seq,
        retention: right_head.retention,
    };
    let mut rejected_doc = right_head.doc.clone();
    rejected_doc.storage_reserved_bytes = 5;
    assert!(matches!(
        right_journal
            .commit(
                "ses_meter_right",
                &mut right_lease,
                &[],
                &rejected_doc,
                right_head.last_seq,
            )
            .await,
        Err(BrainError::TenantStorageQuotaExceeded {
            requested: 5,
            limit: 10
        })
    ));
    assert_eq!(
        right_journal
            .get_head("ses_meter_right")
            .await
            .unwrap()
            .doc
            .tenant_metered_storage_bytes,
        0,
        "a rejected decision leaves the authoritative session contribution unchanged"
    );

    let mut right_doc = right_head.doc;
    right_doc.storage_reserved_bytes = 4;
    right_doc = right_journal
        .commit(
            "ses_meter_right",
            &mut right_lease,
            &[],
            &right_doc,
            right_head.last_seq,
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .tenant_storage
            .lock()
            .expect("tenant meter")
            .get("tenant-meter"),
        Some(&10)
    );

    // Reserve -> publish is a gauge transfer, not a second charge.
    left_doc.session_storage_bytes = 6;
    left_doc.storage_reserved_bytes = 0;
    let left_last_seq = left_lease.last_seq;
    left_doc = left_journal
        .commit(
            "ses_meter_left",
            &mut left_lease,
            &[],
            &left_doc,
            left_last_seq,
        )
        .await
        .unwrap();
    assert_eq!(left_doc.tenant_metered_storage_bytes, 6);
    assert_eq!(
        store
            .tenant_storage
            .lock()
            .expect("tenant meter")
            .get("tenant-meter"),
        Some(&10)
    );

    let status = DeletionStatusDoc {
        session_id: "ses_meter_left".into(),
        tenant_id: "tenant-meter".into(),
        root_id: "ses_meter_left".into(),
        parent_id: None,
        metered_storage_bytes: 6,
        metered_journal_bytes: left_lease.retention.metered_bytes,
        state: "succeeded".into(),
        requested_at_ms: 1,
        updated_at_ms: 2,
        completed_at_ms: Some(2),
        expires_at_ms: u64::MAX,
        attempts: 1,
        last_error: None,
    };
    store.finalize_deletion(&status).await.unwrap();
    store.finalize_deletion(&status).await.unwrap();
    assert_eq!(
        store
            .tenant_storage
            .lock()
            .expect("tenant meter")
            .get("tenant-meter"),
        Some(&4),
        "lost final response cannot release tenant capacity twice"
    );

    // The surviving root can immediately consume the released capacity.
    right_doc.storage_reserved_bytes = 10;
    let right_last_seq = right_lease.last_seq;
    let right_doc = right_journal
        .commit(
            "ses_meter_right",
            &mut right_lease,
            &[],
            &right_doc,
            right_last_seq,
        )
        .await
        .unwrap();
    assert_eq!(right_doc.tenant_metered_storage_bytes, 10);
}

#[tokio::test]
async fn root_bundle_bytes_reserve_tenant_capacity_at_create_and_release_once() {
    let store = Arc::new(MemoryStore::default());
    let journal = Journal::new(store.clone(), "owner-a").with_tenant_storage_limit(10);
    let mut first = head_doc();
    first.tenant_id = "tenant-bundles".into();
    first.root_id = "ses_bundle_first".into();
    first.tenant_metered_storage_bytes = 6;
    journal
        .create(
            "ses_bundle_first",
            &first,
            &Record::State {
                state: "open".into(),
                turn: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(first.session_storage_bytes, 0);
    assert_eq!(first.storage_reserved_bytes, 0);
    assert_eq!(
        store
            .tenant_storage
            .lock()
            .expect("tenant meter")
            .get("tenant-bundles"),
        Some(&6)
    );

    let mut rejected = head_doc();
    rejected.tenant_id = "tenant-bundles".into();
    rejected.root_id = "ses_bundle_rejected".into();
    rejected.tenant_metered_storage_bytes = 5;
    assert!(matches!(
        journal
            .create(
                "ses_bundle_rejected",
                &rejected,
                &Record::State {
                    state: "open".into(),
                    turn: None,
                },
            )
            .await,
        Err(BrainError::TenantStorageQuotaExceeded {
            requested: 5,
            limit: 10
        })
    ));
    assert!(matches!(
        journal.get_head("ses_bundle_rejected").await,
        Err(BrainError::NoSuchSession(_))
    ));

    let status = DeletionStatusDoc {
        session_id: "ses_bundle_first".into(),
        tenant_id: "tenant-bundles".into(),
        root_id: "ses_bundle_first".into(),
        parent_id: None,
        metered_storage_bytes: 6,
        metered_journal_bytes: store
            .get_head("ses_bundle_first")
            .await
            .unwrap()
            .retention
            .metered_bytes,
        state: "succeeded".into(),
        requested_at_ms: 1,
        updated_at_ms: 2,
        completed_at_ms: Some(2),
        expires_at_ms: u64::MAX,
        attempts: 1,
        last_error: None,
    };
    store.finalize_deletion(&status).await.unwrap();
    store.finalize_deletion(&status).await.unwrap();
    assert_eq!(
        store
            .tenant_storage
            .lock()
            .expect("tenant meter")
            .get("tenant-bundles"),
        Some(&0)
    );
    journal
        .create(
            "ses_bundle_rejected",
            &rejected,
            &Record::State {
                state: "open".into(),
                turn: None,
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn final_child_deletion_removes_the_strong_parent_adjacency() {
    let store = MemoryStore::default();
    let mut parent = head_doc();
    parent.root_id = "ses_parent".into();
    create_memory_store(
        &store,
        "ses_parent",
        &parent,
        &Record::State {
            state: "open".into(),
            turn: None,
        },
        "owner-a",
        0,
    )
    .await
    .unwrap();
    let mut child = head_doc();
    child.root_id = "ses_parent".into();
    child.parent_id = Some("ses_parent".into());
    child.ancestor_ids = vec!["ses_parent".into()];
    child.depth = 1;
    create_memory_store(
        &store,
        "ses_child",
        &child,
        &Record::State {
            state: "open".into(),
            turn: None,
        },
        "owner-a",
        0,
    )
    .await
    .unwrap();
    assert_eq!(
        store
            .list_child_page(&ChildListQuery {
                parent_id: "ses_parent",
                limit: 100,
                cursor: None,
            })
            .await
            .unwrap()
            .sessions
            .len(),
        1
    );
    store
        .finalize_deletion(&DeletionStatusDoc {
            session_id: "ses_child".into(),
            tenant_id: "local".into(),
            root_id: "ses_parent".into(),
            parent_id: Some("ses_parent".into()),
            metered_storage_bytes: 0,
            metered_journal_bytes: store
                .get_head("ses_child")
                .await
                .unwrap()
                .retention
                .metered_bytes,
            state: "succeeded".into(),
            requested_at_ms: 1,
            updated_at_ms: 2,
            completed_at_ms: Some(2),
            expires_at_ms: u64::MAX,
            attempts: 1,
            last_error: None,
        })
        .await
        .unwrap();
    assert!(
        store
            .list_child_page(&ChildListQuery {
                parent_id: "ses_parent",
                limit: 100,
                cursor: None,
            })
            .await
            .unwrap()
            .sessions
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn child_admission_is_atomic_at_the_direct_limit_and_releases_once() {
    let store = Arc::new(MemoryStore::default());
    let mut root = head_doc();
    root.root_id = "ses_quota_root".into();
    root.prefix.max_direct_children = 3;
    root.prefix.max_descendants = 3;
    create_memory_store(
        &store,
        "ses_quota_root",
        &root,
        &Record::State {
            state: "open".into(),
            turn: None,
        },
        "owner-a",
        0,
    )
    .await
    .unwrap();

    let mut tasks = Vec::new();
    for index in 0..16 {
        let store = Arc::clone(&store);
        let mut child = root.clone();
        child.root_id = "ses_quota_root".into();
        child.parent_id = Some("ses_quota_root".into());
        child.ancestor_ids = vec!["ses_quota_root".into()];
        child.depth = 1;
        let child_id = format!("ses_quota_child_{index:02}");
        tasks.push(tokio::spawn(async move {
            let result = create_memory_store(
                &store,
                &child_id,
                &child,
                &Record::State {
                    state: "open".into(),
                    turn: None,
                },
                "owner-a",
                0,
            )
            .await;
            (child_id, result)
        }));
    }
    let mut admitted = Vec::new();
    let mut rejected = 0;
    for task in tasks {
        let (child_id, result) = task.await.unwrap();
        match result {
            Ok(()) => admitted.push(child_id),
            Err(BrainError::Overloaded) => rejected += 1,
            Err(error) => panic!("unexpected child admission error: {error}"),
        }
    }
    assert_eq!(admitted.len(), 3);
    assert_eq!(rejected, 13);
    {
        let sessions = store.sessions.lock().expect("memory journal");
        let root = sessions.get("ses_quota_root").unwrap();
        assert_eq!(root.direct_children, 3);
        assert_eq!(root.descendants, 3);
    }

    let released = admitted.pop().unwrap();
    let released_journal_bytes = store
        .get_head(&released)
        .await
        .unwrap()
        .retention
        .metered_bytes;
    let terminal = DeletionStatusDoc {
        session_id: released,
        tenant_id: "local".into(),
        root_id: "ses_quota_root".into(),
        parent_id: Some("ses_quota_root".into()),
        metered_storage_bytes: 0,
        metered_journal_bytes: released_journal_bytes,
        state: "succeeded".into(),
        requested_at_ms: 1,
        updated_at_ms: 2,
        completed_at_ms: Some(2),
        expires_at_ms: u64::MAX,
        attempts: 1,
        last_error: None,
    };
    store.finalize_deletion(&terminal).await.unwrap();
    store
        .finalize_deletion(&terminal)
        .await
        .expect("lost-response retry is idempotent");
    {
        let sessions = store.sessions.lock().expect("memory journal");
        let root = sessions.get("ses_quota_root").unwrap();
        assert_eq!(root.direct_children, 2);
        assert_eq!(root.descendants, 2);
    }

    let mut replacement = root.clone();
    replacement.parent_id = Some("ses_quota_root".into());
    replacement.ancestor_ids = vec!["ses_quota_root".into()];
    replacement.depth = 1;
    create_memory_store(
        &store,
        "ses_quota_replacement",
        &replacement,
        &Record::State {
            state: "open".into(),
            turn: None,
        },
        "owner-a",
        3,
    )
    .await
    .unwrap();

    let claimed = store.claim("ses_quota_root", "owner-a", 4).await.unwrap();
    let mut fenced = claimed.doc;
    fenced.ended = true;
    fenced.state = "ending".into();
    store
        .commit(
            "ses_quota_root",
            "owner-a",
            claimed.fence,
            &[],
            &fenced,
            claimed.last_seq,
            4,
            0,
            u64::MAX,
            claimed.retention,
            0,
            JournalRetentionLimits::default(),
        )
        .await
        .unwrap();
    let mut late = root;
    late.parent_id = Some("ses_quota_root".into());
    late.ancestor_ids = vec!["ses_quota_root".into()];
    late.depth = 1;
    assert!(matches!(
        create_memory_store(
            &store,
            "ses_after_end_fence",
            &late,
            &Record::State {
                state: "open".into(),
                turn: None,
            },
            "owner-b",
            5,
        )
        .await,
        Err(BrainError::Invalid(_))
    ));
}

#[tokio::test]
async fn descendant_limit_is_shared_across_breadth_and_depth() {
    let store = MemoryStore::default();
    let mut root = head_doc();
    root.root_id = "ses_desc_root".into();
    root.prefix.max_direct_children = 8;
    root.prefix.max_descendants = 2;
    create_memory_store(
        &store,
        "ses_desc_root",
        &root,
        &Record::State {
            state: "open".into(),
            turn: None,
        },
        "owner-a",
        0,
    )
    .await
    .unwrap();
    let mut child = root.clone();
    child.parent_id = Some("ses_desc_root".into());
    child.ancestor_ids = vec!["ses_desc_root".into()];
    child.depth = 1;
    create_memory_store(
        &store,
        "ses_desc_child",
        &child,
        &Record::State {
            state: "open".into(),
            turn: None,
        },
        "owner-a",
        1,
    )
    .await
    .unwrap();
    let mut grandchild = root.clone();
    grandchild.parent_id = Some("ses_desc_child".into());
    grandchild.ancestor_ids = vec!["ses_desc_root".into(), "ses_desc_child".into()];
    grandchild.depth = 2;
    create_memory_store(
        &store,
        "ses_desc_grandchild",
        &grandchild,
        &Record::State {
            state: "open".into(),
            turn: None,
        },
        "owner-a",
        2,
    )
    .await
    .unwrap();
    let mut sibling = child;
    sibling.parent_id = Some("ses_desc_root".into());
    sibling.ancestor_ids = vec!["ses_desc_root".into()];
    assert!(matches!(
        create_memory_store(
            &store,
            "ses_desc_sibling",
            &sibling,
            &Record::State {
                state: "open".into(),
                turn: None,
            },
            "owner-a",
            3,
        )
        .await,
        Err(BrainError::Overloaded)
    ));
}

#[tokio::test]
async fn ancestor_fence_atomically_rejects_a_deep_turn_and_new_descendant() {
    let journal = Journal::new_memory("brain-ancestor-race");
    let mut root = head_doc();
    root.root_id = "ses_root".into();
    root.prefix.max_child_depth = 8;
    journal
        .create(
            "ses_root",
            &root,
            &Record::State {
                state: "open".into(),
                turn: None,
            },
        )
        .await
        .unwrap();

    let mut child = root.clone();
    child.root_id = "ses_root".into();
    child.parent_id = Some("ses_root".into());
    child.ancestor_ids = vec!["ses_root".into()];
    child.depth = 1;
    journal
        .create(
            "ses_child",
            &child,
            &Record::State {
                state: "open".into(),
                turn: None,
            },
        )
        .await
        .unwrap();

    let mut grandchild = child.clone();
    grandchild.parent_id = Some("ses_child".into());
    grandchild.ancestor_ids = vec!["ses_root".into(), "ses_child".into()];
    grandchild.depth = 2;
    journal
        .create(
            "ses_grandchild",
            &grandchild,
            &Record::State {
                state: "open".into(),
                turn: None,
            },
        )
        .await
        .unwrap();

    // The deep actor wins its own lease before the root fence, exactly like a concurrent
    // follow-up on another replica. The later decision must still condition the root path.
    let grandchild_head = journal.claim("ses_grandchild").await.unwrap();
    let mut root_head = journal.claim("ses_root").await.unwrap();
    root_head.doc.ended = true;
    root_head.doc.state = "ending".into();
    let mut root_lease = Lease {
        fence: root_head.fence,
        last_seq: root_head.last_seq,
        retention: root_head.retention,
    };
    journal
        .commit(
            "ses_root",
            &mut root_lease,
            &[(
                2,
                Record::State {
                    state: "ending".into(),
                    turn: None,
                },
            )],
            &root_head.doc,
            2,
        )
        .await
        .unwrap();

    let mut grandchild_doc = grandchild_head.doc.clone();
    grandchild_doc.state = "open".into();
    grandchild_doc.turn = Some("trn_after_fence".into());
    let mut grandchild_lease = Lease {
        fence: grandchild_head.fence,
        last_seq: grandchild_head.last_seq,
        retention: grandchild_head.retention,
    };
    assert!(matches!(
        journal
            .commit(
                "ses_grandchild",
                &mut grandchild_lease,
                &[(
                    2,
                    Record::TurnStarted {
                        turn: "trn_after_fence".into(),
                    },
                )],
                &grandchild_doc,
                2,
            )
            .await,
        Err(BrainError::Fenced)
    ));

    let mut great_grandchild = grandchild.clone();
    great_grandchild.parent_id = Some("ses_grandchild".into());
    great_grandchild.ancestor_ids = vec![
        "ses_root".into(),
        "ses_child".into(),
        "ses_grandchild".into(),
    ];
    great_grandchild.depth = 3;
    assert!(matches!(
        journal
            .create(
                "ses_great_grandchild",
                &great_grandchild,
                &Record::State {
                    state: "open".into(),
                    turn: None,
                },
            )
            .await,
        Err(BrainError::Invalid(_))
    ));
}

fn model_intent(turn: &str) -> Record {
    Record::ModelCallIntent {
        turn: turn.into(),
        logical_operation_id: format!("model:{turn}:1"),
        attempt_id: "att_aaaaaaaaaaaaaaaaaaaa".into(),
        request_digest: "a".repeat(64),
        replacement: 0,
    }
}

fn sandbox_status(state: &str) -> brain_protocol::hand::SandboxStatus {
    serde_json::from_value(serde_json::json!({
        "state": state,
        "target": {
            "binding_ref": "binding_default",
            "kind": "default",
            "root_id": "ses_retention",
            "session_id": "ses_retention"
        },
        "expires_at_ms": null
    }))
    .unwrap()
}

#[test]
fn retention_policy_validation_is_ordered_and_adapter_representable() {
    assert!(JournalRetentionLimits::default().validate().is_ok());
    for limits in [
        JournalRetentionLimits {
            session_bytes: MIN_SESSION_JOURNAL_BYTES - 1,
            ..JournalRetentionLimits::default()
        },
        JournalRetentionLimits {
            session_bytes: DEFAULT_MAX_SESSION_JOURNAL_BYTES,
            tenant_bytes: DEFAULT_MAX_SESSION_JOURNAL_BYTES - 1,
            tenant_sessions: DEFAULT_MAX_TENANT_RETAINED_SESSIONS,
        },
        JournalRetentionLimits {
            tenant_sessions: 0,
            ..JournalRetentionLimits::default()
        },
        JournalRetentionLimits {
            session_bytes: MAX_JOURNAL_BYTES + 1,
            tenant_bytes: MAX_JOURNAL_BYTES + 1,
            tenant_sessions: DEFAULT_MAX_TENANT_RETAINED_SESSIONS,
        },
    ] {
        assert!(matches!(limits.validate(), Err(BrainError::Invalid(_))));
    }
    assert!(
        JournalRetentionLimits {
            session_bytes: MIN_SESSION_JOURNAL_BYTES,
            tenant_bytes: MIN_SESSION_JOURNAL_BYTES,
            tenant_sessions: MIN_TENANT_RETAINED_SESSIONS,
        }
        .validate()
        .is_ok()
    );
    assert!(
        JournalRetentionLimits {
            session_bytes: MAX_JOURNAL_BYTES,
            tenant_bytes: MAX_JOURNAL_BYTES,
            tenant_sessions: MAX_TENANT_RETAINED_SESSIONS,
        }
        .validate()
        .is_ok()
    );
}

#[test]
fn every_effect_class_reserves_before_dispatch_and_recovery_does_not_duplicate_it() {
    let first = Record::State {
        state: "open".into(),
        turn: None,
    };
    let initial = initial_retention(&first, u64::MAX).unwrap();
    let single_terminal_intents = vec![
        Record::CompactionIntent {
            turn: "trn_effects".into(),
            logical_operation_id: "compact:trn_effects:1".into(),
            attempt_id: "att_compaction".into(),
            request_digest: "a".repeat(64),
            replacement: 0,
        },
        Record::CustomerCallIntent {
            turn: "trn_effects".into(),
            call: "op_customer".into(),
            client_id: "app".into(),
            process_id: "process:test".into(),
            request_digest: "b".repeat(64),
            deadline_at_ms: 10_000,
        },
        Record::ToolCall {
            turn: "trn_effects".into(),
            agent: "root".into(),
            call: "op_managed".into(),
            name: "managed".into(),
            input: serde_json::json!({"value": true}),
            detach: false,
        },
        Record::StorageUploadReserved {
            transfer_id: "transfer_effects".into(),
            key: "out.bin".into(),
            bytes: 1,
            sha256: Some("c".repeat(64)),
            expires_at_ms: 10_000,
            published_bytes: 0,
            reserved_bytes: 1,
        },
        Record::StorageDeleteIntent {
            operation_id: "delete_effects".into(),
            key: "old.bin".into(),
            bytes: 1,
            sha256: "d".repeat(64),
            published_bytes: 1,
            reserved_bytes: 0,
        },
        Record::DefaultSandboxChanged {
            status: sandbox_status("creating"),
        },
    ];
    for intent in single_terminal_intents {
        let projected = project_retention(initial, &[(2, intent)], u64::MAX).unwrap();
        assert_eq!(
            projected.effect_reserve_bytes,
            JOURNAL_TERMINAL_RESERVE_BYTES
        );
    }

    let provider = project_retention(initial, &[(2, model_intent("trn_retry"))], u64::MAX).unwrap();
    assert_eq!(provider.effect_reserve_bytes, JOURNAL_EFFECT_RESERVE_BYTES);
    let recovery = vec![
        (
            3,
            Record::ModelCallUnknown {
                turn: "trn_retry".into(),
                logical_operation_id: "model:trn_retry:1".into(),
                attempt_id: "att_aaaaaaaaaaaaaaaaaaaa".into(),
                request_digest: "a".repeat(64),
                possibly_duplicated: true,
            },
        ),
        (
            4,
            Record::ModelAttemptSuperseded {
                turn: "trn_retry".into(),
                logical_operation_id: "model:trn_retry:1".into(),
                superseded_attempt_id: "att_aaaaaaaaaaaaaaaaaaaa".into(),
                replacement_attempt_id: "att_bbbbbbbbbbbbbbbbbbbb".into(),
                reason: "unknown".into(),
            },
        ),
        (
            5,
            Record::ModelCallIntent {
                turn: "trn_retry".into(),
                logical_operation_id: "model:trn_retry:1".into(),
                attempt_id: "att_bbbbbbbbbbbbbbbbbbbb".into(),
                request_digest: "a".repeat(64),
                replacement: 1,
            },
        ),
    ];
    let recovered = project_retention(provider, &recovery, u64::MAX).unwrap();
    assert_eq!(recovered.effect_reserve_bytes, JOURNAL_EFFECT_RESERVE_BYTES);
    assert_eq!(
        recovered.metered_bytes - provider.metered_bytes,
        serialized_record_charge(&recovery).unwrap(),
        "replacement recovery charges only its durable records and restores, rather than duplicates, the reserve"
    );

    let reserved = project_retention(
        initial,
        &[(
            2,
            Record::StorageUploadReserved {
                transfer_id: "transfer_adopt".into(),
                key: "adopt.bin".into(),
                bytes: 1,
                sha256: Some("e".repeat(64)),
                expires_at_ms: 10_000,
                published_bytes: 0,
                reserved_bytes: 1,
            },
        )],
        u64::MAX,
    )
    .unwrap();
    let published_record = Record::StorageUploadPublished {
        transfer_id: "transfer_adopt".into(),
        key: "adopt.bin".into(),
        bytes: 1,
        published_bytes: 1,
        reserved_bytes: 1,
    };
    let published =
        project_retention(reserved, &[(3, published_record.clone())], u64::MAX).unwrap();
    assert!(published.effect_reserve_bytes < JOURNAL_TERMINAL_RESERVE_BYTES);
    let republished = project_retention(published, &[(4, published_record)], u64::MAX).unwrap();
    assert_eq!(
        republished.metered_bytes, published.metered_bytes,
        "replayed adoption consumes already-reserved capacity without adding a second reserve"
    );
}

#[test]
fn effect_retention_reserves_provider_completion_and_tool_terminal_decisions() {
    assert_eq!(
        JOURNAL_EFFECT_RESERVE_BYTES,
        2 * MAX_DECISION_SERIALIZED_BYTES as u64
    );
    let first = Record::State {
        state: "open".into(),
        turn: None,
    };
    let initial = initial_retention(&first, u64::MAX).unwrap();
    let intent = model_intent("trn_retention");
    let after_intent = project_retention(initial, &[(2, intent)], u64::MAX).unwrap();
    assert_eq!(
        after_intent.effect_reserve_bytes,
        JOURNAL_EFFECT_RESERVE_BYTES
    );

    let completion = vec![
        (
            3,
            Record::ModelCallCompleted {
                turn: "trn_retention".into(),
                logical_operation_id: "model:trn_retention:1".into(),
                attempt_id: "att_aaaaaaaaaaaaaaaaaaaa".into(),
                request_digest: "a".repeat(64),
            },
        ),
        (
            4,
            Record::ToolCall {
                turn: "trn_retention".into(),
                agent: "root".into(),
                call: "op_retention".into(),
                name: "managed".into(),
                input: serde_json::json!({"value": "x"}),
                detach: false,
            },
        ),
    ];
    let after_completion = project_retention(after_intent, &completion, u64::MAX).unwrap();
    assert_eq!(
        after_completion.effect_reserve_bytes, JOURNAL_TERMINAL_RESERVE_BYTES,
        "the provider terminal releases its half of the reserve but retains one complete Tool-terminal decision"
    );

    let terminal = Record::ToolResult {
        turn: "trn_retention".into(),
        agent: "root".into(),
        call: "op_retention".into(),
        name: "managed".into(),
        outcome: "completed".into(),
        content: "x".repeat(MAX_RECORD_CONTENT_BYTES),
        is_error: false,
        exit_code: Some(0),
        duration_ms: 1,
        truncated: false,
    };
    let after_terminal = project_retention(after_completion, &[(5, terminal)], u64::MAX).unwrap();
    assert_eq!(after_terminal.effect_reserve_bytes, 0);
    assert!(after_terminal.metered_bytes < after_completion.metered_bytes);
}

#[tokio::test]
async fn retained_identity_quota_is_shared_by_roots_and_children_and_released_once() {
    let store = Arc::new(MemoryStore::default());
    let limits = JournalRetentionLimits {
        session_bytes: DEFAULT_MAX_SESSION_JOURNAL_BYTES,
        tenant_bytes: DEFAULT_MAX_TENANT_JOURNAL_BYTES,
        tenant_sessions: 2,
    };
    let journal = Journal::new(store.clone(), "owner-retention").with_retention_limits(limits);
    let first = Record::State {
        state: "open".into(),
        turn: None,
    };
    let mut root = head_doc();
    root.tenant_id = "tenant-retention-identities".into();
    root.root_id = "ses_retained_root".into();
    journal
        .create("ses_retained_root", &root, &first)
        .await
        .unwrap();

    let mut child = root.clone();
    child.parent_id = Some(root.root_id.clone());
    child.ancestor_ids = vec![root.root_id.clone()];
    child.depth = 1;
    journal
        .create("ses_retained_child", &child, &first)
        .await
        .unwrap();
    assert_eq!(
        store
            .tenant_retention
            .lock()
            .expect("retention meter")
            .get(&root.tenant_id)
            .map(|meter| meter.1),
        Some(2)
    );

    let mut rejected = head_doc();
    rejected.tenant_id = root.tenant_id.clone();
    rejected.root_id = "ses_retained_rejected".into();
    assert!(matches!(
        journal
            .create("ses_retained_rejected", &rejected, &first)
            .await,
        Err(BrainError::TenantRetainedSessionQuotaExceeded { limit: 2 })
    ));

    let child_head = journal.get_head("ses_retained_child").await.unwrap();
    let terminal = DeletionStatusDoc {
        session_id: "ses_retained_child".into(),
        tenant_id: root.tenant_id.clone(),
        root_id: root.root_id.clone(),
        parent_id: Some(root.root_id.clone()),
        metered_storage_bytes: 0,
        metered_journal_bytes: child_head.retention.metered_bytes,
        state: "succeeded".into(),
        requested_at_ms: 1,
        updated_at_ms: 2,
        completed_at_ms: Some(2),
        expires_at_ms: u64::MAX,
        attempts: 1,
        last_error: None,
    };
    store.finalize_deletion(&terminal).await.unwrap();
    store.finalize_deletion(&terminal).await.unwrap();
    assert_eq!(
        store
            .tenant_retention
            .lock()
            .expect("retention meter")
            .get(&root.tenant_id)
            .map(|meter| meter.1),
        Some(1),
        "lost final response cannot release a retained identity twice"
    );
    journal
        .create("ses_retained_rejected", &rejected, &first)
        .await
        .expect("physical final deletion immediately frees one identity");
}

#[tokio::test]
async fn journal_quota_rejection_is_atomic_and_end_uses_precharged_lifecycle_capacity() {
    let first = Record::State {
        state: "open".into(),
        turn: None,
    };
    let initial = initial_retention(&first, u64::MAX).unwrap();
    let user = user(
        "trn_session_limit",
        "ordinary append must not consume lifecycle capacity",
    );
    let ordinary_charge = serialized_record_charge(&[(2, user.clone())]).unwrap();
    let limits = JournalRetentionLimits {
        session_bytes: initial
            .metered_bytes
            .saturating_add(ordinary_charge)
            .saturating_sub(1),
        tenant_bytes: u64::MAX,
        tenant_sessions: 8,
    };
    let store = Arc::new(MemoryStore::default());
    let journal = Journal::new(store.clone(), "owner-session-limit").with_retention_limits(limits);
    let mut doc = head_doc();
    doc.root_id = "ses_session_limit".into();
    journal
        .create("ses_session_limit", &doc, &first)
        .await
        .unwrap();
    let head = journal.claim("ses_session_limit").await.unwrap();
    let mut lease = Lease {
        fence: head.fence,
        last_seq: head.last_seq,
        retention: head.retention,
    };
    assert!(matches!(
        journal
            .commit("ses_session_limit", &mut lease, &[(2, user)], &head.doc, 2,)
            .await,
        Err(BrainError::SessionJournalQuotaExceeded { .. })
    ));
    let persisted = journal.get_head("ses_session_limit").await.unwrap();
    assert_eq!(persisted.last_seq, 1);
    assert_eq!(persisted.retention, initial);
    assert_eq!(lease.last_seq, 1);
    assert_eq!(lease.retention, initial);

    let fenced = journal
        .fence_end("ses_session_limit")
        .await
        .expect("END consumes its create-time reserve even at the ordinary append ceiling");
    assert!(fenced.newly_fenced);
    assert_eq!(fenced.head.doc.state, "ending");
    assert_eq!(fenced.head.retention.metered_bytes, initial.metered_bytes);
    assert!(fenced.head.retention.lifecycle_reserve_bytes < initial.lifecycle_reserve_bytes);
}

#[tokio::test]
async fn effect_terminal_commits_without_new_quota_after_intent_reservation() {
    let first = Record::State {
        state: "open".into(),
        turn: None,
    };
    let initial = initial_retention(&first, u64::MAX).unwrap();
    let intent = model_intent("trn_effect_reserve");
    let after_intent = project_retention(initial, &[(2, intent.clone())], u64::MAX).unwrap();
    let limits = JournalRetentionLimits {
        session_bytes: after_intent.metered_bytes,
        tenant_bytes: after_intent.metered_bytes,
        tenant_sessions: 1,
    };
    let store = Arc::new(MemoryStore::default());
    let journal = Journal::new(store, "owner-effect-reserve").with_retention_limits(limits);
    let mut doc = head_doc();
    doc.root_id = "ses_effect_reserve".into();
    journal
        .create("ses_effect_reserve", &doc, &first)
        .await
        .unwrap();
    let head = journal.claim("ses_effect_reserve").await.unwrap();
    let mut lease = Lease {
        fence: head.fence,
        last_seq: head.last_seq,
        retention: head.retention,
    };
    journal
        .commit(
            "ses_effect_reserve",
            &mut lease,
            &[(2, intent)],
            &head.doc,
            2,
        )
        .await
        .expect("intent atomically reserves every later terminal byte");
    assert_eq!(lease.retention, after_intent);

    let completed = vec![
        (
            3,
            Record::ModelCallCompleted {
                turn: "trn_effect_reserve".into(),
                logical_operation_id: "model:trn_effect_reserve:1".into(),
                attempt_id: "att_aaaaaaaaaaaaaaaaaaaa".into(),
                request_digest: "a".repeat(64),
            },
        ),
        (
            4,
            Record::ToolCall {
                turn: "trn_effect_reserve".into(),
                agent: "root".into(),
                call: "op_effect_reserve".into(),
                name: "managed".into(),
                input: serde_json::json!({"value": true}),
                detach: false,
            },
        ),
    ];
    journal
        .commit("ses_effect_reserve", &mut lease, &completed, &head.doc, 4)
        .await
        .expect("provider terminal consumes only the already-reserved capacity");
    let tool_result = Record::ToolResult {
        turn: "trn_effect_reserve".into(),
        agent: "root".into(),
        call: "op_effect_reserve".into(),
        name: "managed".into(),
        outcome: "completed".into(),
        content: "terminal".into(),
        is_error: false,
        exit_code: Some(0),
        duration_ms: 1,
        truncated: false,
    };
    journal
        .commit(
            "ses_effect_reserve",
            &mut lease,
            &[(5, tool_result)],
            &head.doc,
            5,
        )
        .await
        .expect("executed Tool terminal stays journalable at the tenant/session ceiling");
    assert_eq!(lease.retention.effect_reserve_bytes, 0);
}

#[tokio::test]
async fn recovery_cursor_reaches_later_due_rows_while_early_rows_remain_due() {
    let store = MemoryStore::default();
    let shard = "r00";
    let mut ids = Vec::new();
    for candidate in 0..20_000u32 {
        let id = format!("ses_cursor_{candidate:08}");
        if recovery_shard(&id) == shard {
            ids.push(id);
            if ids.len() == 40 {
                break;
            }
        }
    }
    assert_eq!(ids.len(), 40);
    for (index, id) in ids.iter().enumerate() {
        let mut doc = head_doc();
        doc.root_id = id.clone();
        doc.state = "open".into();
        doc.turn = Some(format!("trn_cursor_{index:08}"));
        doc.active_phase = Some("model_running".into());
        doc.recovery_due_ms = Some(1);
        create_memory_store(
            &store,
            id,
            &doc,
            &Record::State {
                state: "open".into(),
                turn: doc.turn.clone(),
            },
            "owner-a",
            0,
        )
        .await
        .unwrap();
    }
    let first = store
        .list_recovery_page(&RecoveryQuery {
            shard,
            due_before_ms: 1,
            limit: 32,
            cursor: None,
        })
        .await
        .unwrap();
    assert_eq!(first.items.len(), 32);
    let cursor = first.next_cursor.expect("more due rows");
    let second = store
        .list_recovery_page(&RecoveryQuery {
            shard,
            due_before_ms: 1,
            limit: 32,
            cursor: Some(&cursor),
        })
        .await
        .unwrap();
    assert_eq!(second.items.len(), 8);
    assert!(second.next_cursor.is_none());
    assert!(
        first
            .items
            .iter()
            .all(|item| item.due_ms == 1 && item.state == "open")
    );
}
