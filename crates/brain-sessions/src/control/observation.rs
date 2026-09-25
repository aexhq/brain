use super::*;

impl EnvironmentRegistry {
    pub fn on_observation(
        &self,
        listener: Option<ObservationListener>,
        maximum_emitted_bytes: usize,
    ) {
        *self
            .control
            .observed
            .lock()
            .expect("Environment observation listener poisoned") = listener;
        self.control
            .maximum_emitted_bytes
            .store(maximum_emitted_bytes, Ordering::Release);
    }

    pub async fn report(
        &self,
        store: Arc<dyn SessionStore>,
        reference: &EnvironmentRef,
        event: brain_protocol::EnvironmentEvent,
    ) -> Result<u64, Error> {
        self.report_operation(store, reference, event, None).await
    }

    pub(super) async fn report_operation(
        &self,
        store: Arc<dyn SessionStore>,
        reference: &EnvironmentRef,
        event: brain_protocol::EnvironmentEvent,
        operation: Option<u64>,
    ) -> Result<u64, Error> {
        let lock = self
            .control
            .locks
            .acquire(store.session_id().clone())
            .map_err(|error| Error::Journal(error.message))?;
        let guard = lock.lock().await;
        let config = brain::session_config(&*store)?;
        let views = brain::environment::environments(&*store, &config)?;
        let closing_operation = operation.is_some()
            && views.get(&reference.name).is_some_and(|view| {
                view.reference == *reference && view.pending_operation == operation
            });
        let resolution = matches!(&event, brain_protocol::EnvironmentEvent::Result { output }
            if matches!(output.observation, brain_protocol::EnvironmentObservation::Operation { .. }));
        if !views.get(&reference.name).is_some_and(|view| {
            view.reference == *reference
                && (view.state != EnvironmentState::Deleting || resolution || closing_operation)
                && !matches!(
                    view.state,
                    EnvironmentState::Deleted | EnvironmentState::Detached
                )
        }) {
            return Err(Error::NotFound(
                "Environment binding is closed or stale".into(),
            ));
        }
        if !closing_operation
            && matches!(
                store.session_summary()?.status,
                brain_protocol::SessionStatus::Ended
                    | brain_protocol::SessionStatus::Ending
                    | brain_protocol::SessionStatus::Failed
            )
        {
            return Err(Error::InvalidState(
                "session no longer accepts Environment observations".into(),
            ));
        }
        let actionable = matches!(&event, brain_protocol::EnvironmentEvent::Result { .. });
        let (kind, payload) = match event {
            brain_protocol::EnvironmentEvent::Result { output } => {
                (codes::event::ENVIRONMENT_OBSERVATION.into(), json!(output))
            }
            brain_protocol::EnvironmentEvent::Event { event_type, data } => {
                if !crate::service::valid_identifier(&event_type)
                    || codes::event::ALL.contains(&event_type.as_str())
                    || event_type == "_extension_event"
                {
                    return Err(Error::InvalidState(
                        "Environment diagnostics cannot impersonate kernel records".into(),
                    ));
                }
                (event_type, data)
            }
        };
        let maximum = self.control.maximum_emitted_bytes.load(Ordering::Acquire);
        if maximum != 0 {
            let mut size = kind.len() + payload.to_string().len();
            let mut after = store
                .fold()?
                .kv
                .get(brain::LAST_ACTIVATION_KEY)
                .and_then(Value::as_u64)
                .unwrap_or(0);
            loop {
                if size > maximum {
                    return Err(Error::EmitLimit(
                        "Environment observations exceed the pending Event budget".into(),
                    ));
                }
                let records = store.records_after(after, 1000)?;
                if records.is_empty() {
                    break;
                }
                for record in records {
                    after = record.sequence;
                    if record.origin
                        == Some(brain_protocol::EventOrigin::Environment {
                            environment: reference.clone(),
                        })
                    {
                        size = size
                            .saturating_add(record.kind.len() + record.payload.to_string().len());
                    }
                }
            }
        }
        let record = AppendRecord {
            kind,
            payload,
            origin: Some(brain_protocol::EventOrigin::Environment {
                environment: reference.clone(),
            }),
        };
        let mut records = Vec::new();
        if actionable
            && let brain_protocol::EnvironmentObservation::Operation {
                sequence,
                resolution,
            } = serde_json::from_value(record.payload["observation"].clone())
                .map_err(|error| Error::InvalidState(error.to_string()))?
        {
            let view = &views[&reference.name];
            if view.pending_operation != Some(sequence)
                || !matches!(
                    view.state,
                    EnvironmentState::Starting
                        | EnvironmentState::Deleting
                        | EnvironmentState::Unknown
                )
            {
                return Err(Error::Conflict(
                    "Environment operation is already settled".into(),
                ));
            }
            let started = store
                .records_after(sequence.saturating_sub(1), 1)?
                .into_iter()
                .next()
                .filter(|record| {
                    record.sequence == sequence
                        && record.payload["environment"].as_str() == Some(reference.name.as_str())
                })
                .ok_or_else(|| {
                    Error::InvalidState(
                        "observation does not name this Environment's operation".into(),
                    )
                })?;
            let operation_ref = if started.kind == codes::event::ENVIRONMENT_REPLACE_STARTED {
                sequence
            } else {
                started.payload["environment_sequence"]
                    .as_u64()
                    .unwrap_or(1)
            };
            if operation_ref != reference.sequence {
                return Err(Error::InvalidState(
                    "observation names an old Environment incarnation".into(),
                ));
            }
            use brain_protocol::EnvironmentResolution as Resolution;
            let ended = match (started.kind.as_str(), &resolution) {
                (codes::event::ENVIRONMENT_SETUP_STARTED, Resolution::Ready) => {
                    codes::event::ENVIRONMENT_SETUP_ENDED
                }
                (codes::event::ENVIRONMENT_REPLACE_STARTED, Resolution::Ready) => {
                    codes::event::ENVIRONMENT_REPLACE_ENDED
                }
                (codes::event::ENVIRONMENT_DETACH_STARTED, Resolution::Detached) => {
                    codes::event::ENVIRONMENT_DETACH_ENDED
                }
                (codes::event::ENVIRONMENT_TEARDOWN_STARTED, Resolution::Deleted) => {
                    codes::event::ENVIRONMENT_TEARDOWN_ENDED
                }
                (codes::event::ENVIRONMENT_SETUP_STARTED, Resolution::Failed { .. }) => {
                    codes::event::ENVIRONMENT_SETUP_FAILED
                }
                (codes::event::ENVIRONMENT_REPLACE_STARTED, Resolution::Failed { .. }) => {
                    codes::event::ENVIRONMENT_REPLACE_FAILED
                }
                (codes::event::ENVIRONMENT_DETACH_STARTED, Resolution::Failed { .. }) => {
                    codes::event::ENVIRONMENT_DETACH_FAILED
                }
                (codes::event::ENVIRONMENT_TEARDOWN_STARTED, Resolution::Failed { .. }) => {
                    codes::event::ENVIRONMENT_TEARDOWN_FAILED
                }
                _ => {
                    return Err(Error::InvalidState(
                        "resolution does not match the Environment operation".into(),
                    ));
                }
            };
            let payload = match resolution {
                Resolution::Failed { error } => {
                    json!({"sequence": sequence, "code": error.code, "message": error.message, "retryable": error.retryable, "details": error.details, "ambiguous": false})
                }
                _ => json!({"sequence": sequence, "result": {"type":"accepted"}}),
            };
            records.push(AppendRecord::new(ended, payload));
        }
        records.push(record);
        let sequence = store
            .append_sync(&records, SessionUpdate::default())?
            .last()
            .expect("observation record")
            .sequence;
        drop(guard);
        if actionable
            && let Some(listener) = self
                .control
                .observed
                .lock()
                .expect("Environment observation listener poisoned")
                .clone()
        {
            listener(store.session_id().clone(), sequence);
        }
        Ok(sequence)
    }
}
