use super::*;

/// Canonical public file path. The URL surface is deliberately narrower than environment tool paths:
/// only absolute POSIX paths beneath `/workspace` are accepted.
pub fn normalize_workspace_path(path: &str) -> Result<String> {
    if path.len() > 4096 {
        return Err(BrainError::Invalid(
            "file path exceeds 4096 UTF-8 bytes".into(),
        ));
    }
    if path.contains('\0') || path.contains('\\') {
        return Err(BrainError::Invalid(
            "file path contains a forbidden character".into(),
        ));
    }
    let mut parts = path.split('/');
    if parts.next() != Some("") || parts.next() != Some("workspace") {
        return Err(BrainError::Invalid(
            "file path must be absolute and beneath /workspace".into(),
        ));
    }
    let mut clean = Vec::new();
    for part in parts {
        match part {
            "" => continue,
            "." | ".." => {
                return Err(BrainError::Invalid(
                    "file path may not contain . or .. components".into(),
                ));
            }
            value => clean.push(value),
        }
    }
    Ok(if clean.is_empty() {
        "/workspace".into()
    } else {
        format!("/workspace/{}", clean.join("/"))
    })
}

pub fn dialect_of(provider: &str) -> Dialect {
    match provider {
        "anthropic" => Dialect::AnthropicMessages,
        _ => Dialect::OpenAiChat,
    }
}

/// Rebuilds the sealed prefix from the HEAD prefix doc. Deterministic: the same doc always
/// seals to the same digest.
pub fn build_prefix(
    p: &PrefixDoc,
    max_rounds: u32,
) -> Result<(crate::Shared<crate::config::SealedPrefix>, Dialect)> {
    // Components receive Brain's neutral message projection; the internal dialect field remains
    // only until the legacy provider codecs are deleted.
    let dialect = Dialect::OpenAiChat;
    let mut decls = crate::tools::resolve(&p.tools)?;
    for decl in &mut decls {
        if let crate::config::ToolRoute::Intrinsic(capability) = &decl.route
            && !crate::tools::is_direct_engine_capability(capability)
        {
            let policy = p
                .official_capabilities
                .get(capability)
                .cloned()
                .ok_or_else(|| {
                    BrainError::Journal(format!(
                        "sealed official capability {capability} has no trusted policy"
                    ))
                })?;
            decl.route = crate::config::ToolRoute::Server(policy);
        }
    }
    let mut def = AgentDef::new(
        p.system_prompt
            .clone()
            .unwrap_or_else(default_system_prompt),
        p.model.clone(),
        dialect,
    );
    for d in decls {
        def = def.tool(d);
    }
    def = def.sampling(GenOpts {
        max_tokens: u32::try_from(p.max_output_tokens.unwrap_or(4096)).map_err(|_| {
            BrainError::Journal("sealed max_output_tokens exceeds the canonical u32 bound".into())
        })?,
        output_token_parameter: OutputTokenParameter::MaxTokens,
        temperature: p.temperature.map(|t| t as f32),
        reasoning_effort: p.reasoning_effort.clone(),
        stop_sequences: Vec::new(),
    });
    def = def.limits(crate::config::Limits {
        max_rounds,
        ..crate::config::Limits::default()
    });
    let rendered_base = if p.rendered_base.is_null() {
        None
    } else {
        let digest = hex::encode(Sha256::digest(serde_jcs::to_vec(&p.rendered_base)?));
        if digest != p.rendered_base_digest {
            return Err(BrainError::Journal(
                "stored provider base segment digest does not match".into(),
            ));
        }
        Some(p.rendered_base.clone())
    };
    let sealed = def.seal().with_provider_base(
        rendered_base,
        (!p.prompt_cache_key.is_empty()).then(|| p.prompt_cache_key.clone()),
    );
    Ok((sealed, dialect))
}

/// A child session as the `children` capability addresses it. Five of that contract's seven
/// functions take a `child-id`, but the session projection names the identity `id`, beside the
/// caller's own `name` — so the handle a caller must supply was never a key it was given.
pub fn child_doc(child: &session::Session) -> Result<serde_json::Value> {
    let mut value = serde_json::to_value(child)?;
    let object = value.as_object_mut().ok_or_else(|| {
        BrainError::Journal("a child session did not project as an object".into())
    })?;
    let id = object
        .get("id")
        .cloned()
        .ok_or_else(|| BrainError::Journal("a child session projection has no id".into()))?;
    object.insert("child_id".into(), id);
    Ok(value)
}

/// The same handle, for a page of children.
pub fn child_docs(children: &[session::Session]) -> Result<Vec<serde_json::Value>> {
    children.iter().map(child_doc).collect()
}

/// Builds the contract Session document from the head. A sealed value that no longer parses
/// as its contract type is journal corruption and errors loudly, naming the field — the REST
/// read never substitutes placeholders or omits sealed identity.
pub fn session_doc(session_id: &str, doc: &HeadDoc) -> Result<session::Session> {
    let corrupt = |what: &str| {
        BrainError::Journal(format!(
            "session {session_id}: journaled {what} violates the public contract"
        ))
    };
    let agentloop = doc
        .prefix
        .agentloop
        .as_ref()
        .map(|selector| -> Result<session::AgentloopInfo> {
            Ok(session::AgentloopInfo {
                component_digest: selector
                    .component_digest
                    .parse()
                    .map_err(|_| corrupt("agentloop component digest"))?,
                world: selector
                    .world
                    .parse()
                    .map_err(|_| corrupt("agentloop world"))?,
                config: selector.config.clone(),
            })
        })
        .transpose()?;
    Ok(session::Session {
        agentloop,
        context_fork: doc.context_fork.as_ref().map(public_context_fork),
        created_at: crate::events::ts(doc.created_ms),
        current_turn: doc
            .turn
            .as_deref()
            .map(|t| t.parse().map_err(|_| corrupt("turn id")))
            .transpose()?,
        failure: doc.failure.as_ref().map(|f| session::SessionFailure {
            at: crate::events::ts(f.at_ms),
            code: match f.code.as_str() {
                "binding_conflict" => session::SessionFailureCode::BindingConflict,
                "provider_unusable" => session::SessionFailureCode::ProviderUnusable,
                "environment_unavailable" => session::SessionFailureCode::EnvironmentUnavailable,
                _ => session::SessionFailureCode::Internal,
            },
            message: f.message.clone(),
        }),
        id: session_id.parse().map_err(|_| corrupt("session id"))?,
        parent_id: doc
            .parent_id
            .as_deref()
            .map(|id| id.parse().map_err(|_| corrupt("parent session id")))
            .transpose()?,
        retain_until: crate::events::ts(doc.retain_until_ms),
        root_id: doc
            .root_id
            .parse()
            .map_err(|_| corrupt("root session id"))?,
        depth: i64::from(doc.depth),
        environments: environment_names(crate::journal::sorted_environment_names(
            &doc.prefix.environments,
        ))?,
        last_seq: doc.last_seq,
        last_message_at: doc.last_message_ms.map(crate::events::ts),
        metadata: doc
            .prefix
            .metadata
            .iter()
            .map(|(key, value)| {
                (
                    key.as_str()
                        .parse()
                        .expect("sealed metadata key satisfies the public contract"),
                    value
                        .as_str()
                        .parse()
                        .expect("sealed metadata value satisfies the public contract"),
                )
            })
            .collect(),
        model: {
            let selector = doc.prefix.model_component.as_ref().ok_or_else(|| {
                BrainError::Journal("stored session has no Model component selector".into())
            })?;
            session::ModelInfo {
                base_url: doc.prefix.base_url.clone(),
                component_digest: selector
                    .component_digest
                    .parse()
                    .map_err(|_| corrupt("Model component digest"))?,
                context_window_tokens: i64::from(doc.prefix.context_window_tokens),
                name: doc.prefix.model.clone(),
                provider: doc
                    .prefix
                    .provider
                    .parse()
                    .map_err(|_| corrupt("provider"))?,
                world: selector.world.clone(),
            }
        },
        name: doc
            .child_name
            .as_deref()
            .map(|name| name.parse().map_err(|_| corrupt("child name")))
            .transpose()?,
        object: session::SessionObject::Session,
        state: crate::events::session_state(doc.state),
        turn_phase: doc
            .active_phase
            .map(|phase| phase.as_str().parse().map_err(|_| corrupt("turn phase")))
            .transpose()?,
        turn_state: crate::events::session_turn_state(doc.turn.as_deref()),
        shape: doc.prefix.shape.clone(),
        storage: session::StorageInfo {
            session_storage_bytes: doc.session_storage_bytes,
            upload_reserved_bytes: doc.storage_reserved_bytes,
        },
        turns: doc.turns,
        updated_at: crate::events::ts(doc.updated_ms),
    })
}

pub(super) fn session_doc_summary(
    summary: &crate::journal::SessionSummary,
) -> Result<session::Session> {
    let corrupt =
        |what: &str| BrainError::Journal(format!("stored session summary has a corrupt {what}"));
    Ok(session::Session {
        // The bounded listing summary does not carry the sealed prefix; the full session
        // resource does. Absent here, never a fabricated default.
        agentloop: None,
        context_fork: summary.context_fork.as_ref().map(public_context_fork),
        created_at: crate::events::ts(summary.created_ms),
        current_turn: summary.turn.as_deref().and_then(|turn| turn.parse().ok()),
        failure: summary
            .failure
            .as_ref()
            .map(|failure| session::SessionFailure {
                at: crate::events::ts(failure.at_ms),
                code: match failure.code.as_str() {
                    "binding_conflict" => session::SessionFailureCode::BindingConflict,
                    "provider_unusable" => session::SessionFailureCode::ProviderUnusable,
                    "environment_unavailable" => {
                        session::SessionFailureCode::EnvironmentUnavailable
                    }
                    _ => session::SessionFailureCode::Internal,
                },
                message: failure.message.clone(),
            }),
        id: summary
            .session_id
            .parse()
            .map_err(|_| corrupt("session id"))?,
        parent_id: summary
            .parent_id
            .as_deref()
            .map(|id| id.parse().map_err(|_| corrupt("parent id")))
            .transpose()?,
        retain_until: crate::events::ts(summary.retain_until_ms),
        root_id: summary.root_id.parse().map_err(|_| corrupt("root id"))?,
        depth: i64::from(summary.depth),
        environments: environment_names(summary.environments.clone())?,
        last_seq: summary.last_seq,
        last_message_at: summary.last_message_ms.map(crate::events::ts),
        metadata: summary
            .metadata
            .iter()
            .map(|(key, value)| {
                (
                    key.as_str()
                        .parse()
                        .expect("listed metadata key satisfies the public contract"),
                    value
                        .as_str()
                        .parse()
                        .expect("listed metadata value satisfies the public contract"),
                )
            })
            .collect(),
        model: session::ModelInfo {
            base_url: summary.base_url.clone(),
            component_digest: summary
                .model_component_digest
                .as_deref()
                .ok_or_else(|| corrupt("Model component digest"))?
                .parse()
                .map_err(|_| corrupt("Model component digest"))?,
            context_window_tokens: i64::from(summary.context_window_tokens),
            name: summary.model.clone(),
            provider: summary.provider.parse().map_err(|_| corrupt("provider"))?,
            world: summary
                .model_world
                .clone()
                .ok_or_else(|| corrupt("Model world"))?,
        },
        name: summary
            .child_name
            .as_deref()
            .and_then(|name| name.parse().ok()),
        object: session::SessionObject::Session,
        state: crate::events::session_state(summary.state),
        turn_phase: summary.active_phase.map(|phase| {
            phase
                .as_str()
                .parse()
                .expect("the protocol turn-phase vocabulary covers every journal phase")
        }),
        turn_state: crate::events::session_turn_state(summary.turn.as_deref()),
        shape: summary.shape.clone(),
        storage: session::StorageInfo {
            session_storage_bytes: summary.session_storage_bytes,
            upload_reserved_bytes: summary.storage_reserved_bytes,
        },
        turns: summary.turns,
        updated_at: crate::events::ts(summary.updated_ms),
    })
}

fn environment_names(names: Vec<String>) -> Result<Vec<session::EnvironmentName>> {
    names
        .into_iter()
        .map(|name| {
            name.parse().map_err(|_| {
                BrainError::Journal("stored session has a corrupt Environment name".into())
            })
        })
        .collect()
}

pub(super) fn public_context_fork(fork: &ContextForkDoc) -> session::ContextFork {
    session::ContextFork {
        last_n: fork
            .last_n
            .and_then(|turns| std::num::NonZeroU64::new(u64::from(turns))),
        mode: match fork.mode.as_str() {
            "all" => session::ContextForkMode::All,
            "none" => session::ContextForkMode::None,
            "last_n" => session::ContextForkMode::LastN,
            _ => panic!("sealed child context fork mode is a closed enum"),
        },
        resolved_turns: u64::from(fork.resolved_turns),
        source_context_generation: fork.source_context_generation,
        source_projection_digest: fork
            .source_projection_digest
            .parse()
            .expect("sealed child context fork digest satisfies the public contract"),
        source_session_id: fork
            .source_session_id
            .parse()
            .expect("sealed child context fork source id satisfies the public contract"),
        source_through_sequence: fork.source_through_sequence,
    }
}
