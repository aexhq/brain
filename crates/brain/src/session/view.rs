use super::*;

pub fn provider_name(p: &ApiProvider) -> &'static str {
    match p {
        ApiProvider::Openai => "openai",
        ApiProvider::Anthropic => "anthropic",
        ApiProvider::Deepseek => "deepseek",
        ApiProvider::Moonshot => "moonshot",
        ApiProvider::Xai => "xai",
        ApiProvider::OpenaiCompatible => "openai_compatible",
    }
}

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

/// Certified: openai, anthropic. Available uncertified: the rest (they speak one of the two
/// dialects). `openai_compatible` requires an explicit base_url.
pub fn resolve_base_url(p: &ApiProvider, base_url: Option<&str>) -> Result<String> {
    if let Some(u) = base_url {
        if !u.starts_with("https://") {
            return Err(BrainError::Invalid("model.base_url must be https".into()));
        }
        return Ok(u.trim_end_matches('/').to_string());
    }
    Ok(match p {
        ApiProvider::Openai => "https://api.openai.com".into(),
        ApiProvider::Anthropic => "https://api.anthropic.com".into(),
        ApiProvider::Deepseek => "https://api.deepseek.com".into(),
        ApiProvider::Moonshot => "https://api.moonshot.ai".into(),
        ApiProvider::Xai => "https://api.x.ai".into(),
        ApiProvider::OpenaiCompatible => {
            return Err(BrainError::Invalid(
                "model.base_url is required for provider openai_compatible".into(),
            ));
        }
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
    let dialect = dialect_of(&p.provider);
    if dialect == Dialect::AnthropicMessages && p.reasoning_effort.is_some() {
        return Err(BrainError::Invalid(
            "model.reasoning_effort is not supported by the Anthropic MVP profile".into(),
        ));
    }
    let mut decls = crate::tools::resolve(&p.tools)?;
    for decl in &mut decls {
        if let crate::config::ToolRoute::Intrinsic(capability) = &decl.route {
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
        output_token_parameter: output_token_parameter(&p.provider),
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
                source_bundle_sha256: selector
                    .source_bundle_sha256
                    .parse()
                    .map_err(|_| corrupt("agentloop bundle digest"))?,
                toolchain: selector
                    .toolchain
                    .parse()
                    .map_err(|_| corrupt("agentloop toolchain"))?,
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
        root_id: doc
            .root_id
            .parse()
            .map_err(|_| corrupt("root session id"))?,
        depth: i64::from(doc.depth),
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
        model: session::ModelInfo {
            base_url: doc.prefix.base_url.clone(),
            context_window_tokens: i64::from(doc.prefix.context_window_tokens),
            name: doc.prefix.model.clone(),
            provider: match doc.prefix.provider.as_str() {
                "openai" => ApiProvider::Openai,
                "anthropic" => ApiProvider::Anthropic,
                "deepseek" => ApiProvider::Deepseek,
                "moonshot" => ApiProvider::Moonshot,
                "xai" => ApiProvider::Xai,
                _ => ApiProvider::OpenaiCompatible,
            },
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
        root_id: summary.root_id.parse().map_err(|_| corrupt("root id"))?,
        depth: i64::from(summary.depth),
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
            context_window_tokens: i64::from(summary.context_window_tokens),
            name: summary.model.clone(),
            provider: match summary.provider.as_str() {
                "openai" => ApiProvider::Openai,
                "anthropic" => ApiProvider::Anthropic,
                "deepseek" => ApiProvider::Deepseek,
                "moonshot" => ApiProvider::Moonshot,
                "xai" => ApiProvider::Xai,
                _ => ApiProvider::OpenaiCompatible,
            },
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
