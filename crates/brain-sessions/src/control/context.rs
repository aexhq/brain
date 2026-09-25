use super::*;

impl EnvironmentRegistry {
    pub(crate) async fn send(
        &self,
        environment: &brain_protocol::Environment,
        operation: &EnvironmentOperation,
        inner: brain::environment::Services,
    ) -> Result<EnvironmentReceipt, Error> {
        let store = self.store(&operation.session_id)?;
        let config = brain::session_config(&*store)?;
        let views = brain::environment::environments(&*store, &config)?;
        let view = views
            .get(&operation.environment)
            .ok_or_else(|| Error::NotFound("Environment is not declared".into()))?;
        if operation
            .binding
            .as_ref()
            .is_some_and(|reference| reference != &view.reference)
        {
            return Err(Error::Conflict(
                "Environment incarnation changed before delivery".into(),
            ));
        }
        if inner.as_ref().is_some_and(|services| services.cancelled()) {
            return Err(Error::Cancelled(
                "execution was cancelled before Environment delivery".into(),
            ));
        }
        let closed = Arc::new(AtomicBool::new(false));
        let _lifetime = OperationLifetime(closed.clone());
        let controller = Arc::new(Controller {
            registry: self.clone(),
            store,
            reference: view.reference.clone(),
            sequence: operation.sequence,
            grants: environment.environments.clone(),
            closed: closed.clone(),
            execution: inner.clone(),
        });
        let services = Arc::new(Invocation { inner, controller });
        let mut operation = operation.clone();
        operation.template = Some(view.template.clone());
        operation.binding = Some(view.reference.clone());
        operation.configuration = environment.configuration.clone();
        let result = self
            .adapter
            .execute(environment, &operation, Some(services))
            .await;
        closed.store(true, Ordering::Release);
        result
    }
    pub fn track(&self, store: Arc<dyn SessionStore>) {
        let mut stores = self
            .control
            .stores
            .lock()
            .expect("Environment store table poisoned");
        stores.retain(|_, store| store.strong_count() > 0);
        stores.insert(store.session_id().clone(), Arc::downgrade(&store));
    }

    pub(crate) fn store(&self, session: &SessionId) -> Result<Arc<dyn SessionStore>, Error> {
        self.control
            .stores
            .lock()
            .expect("Environment store table poisoned")
            .get(session)
            .and_then(Weak::upgrade)
            .ok_or_else(|| Error::NotFound("Environment session is not open".into()))
    }
}

struct Controller {
    registry: EnvironmentRegistry,
    store: Arc<dyn SessionStore>,
    reference: EnvironmentRef,
    sequence: u64,
    grants: Vec<EnvironmentGrant>,
    closed: Arc<AtomicBool>,
    execution: brain::environment::Services,
}

struct OperationLifetime(Arc<AtomicBool>);
impl Drop for OperationLifetime {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[async_trait]
impl ExecutionServices for Controller {
    fn methods(&self) -> Vec<&'static str> {
        vec!["environments", "emit", "result"]
    }
    async fn call(&self, method: &str, input: Value) -> Result<Value, Error> {
        if self.cancelled() {
            return Err(Error::Cancelled("Environment operation is closed".into()));
        }
        match method {
            "environments" => {
                self.registry
                    .control(
                        self.store.clone(),
                        Some(&self.reference.name),
                        &self.grants,
                        serde_json::from_value(input)
                            .map_err(|error| Error::InvalidState(error.to_string()))?,
                    )
                    .await
            }
            "result" => Ok(json!(
                self.registry
                    .report_operation(
                        self.store.clone(),
                        &self.reference,
                        brain_protocol::EnvironmentEvent::Result {
                            output: serde_json::from_value(input)
                                .map_err(|error| Error::InvalidState(error.to_string()))?
                        },
                        Some(self.sequence),
                    )
                    .await?
            )),
            "emit" => {
                let event: brain_protocol::TurnEmitRequest = serde_json::from_value(input)
                    .map_err(|error| Error::InvalidState(error.to_string()))?;
                Ok(json!(
                    self.registry
                        .report_operation(
                            self.store.clone(),
                            &self.reference,
                            brain_protocol::EnvironmentEvent::Event {
                                event_type: event.event_type,
                                data: event.data,
                            },
                            Some(self.sequence),
                        )
                        .await?
                ))
            }
            _ => Err(denied()),
        }
    }
    fn cancelled(&self) -> bool {
        self.closed.load(Ordering::Acquire)
            || self
                .execution
                .as_ref()
                .is_some_and(|services| services.cancelled())
    }
}

struct Invocation {
    inner: brain::environment::Services,
    controller: Arc<Controller>,
}

#[async_trait]
impl ExecutionServices for Invocation {
    fn methods(&self) -> Vec<&'static str> {
        self.inner
            .as_ref()
            .map_or_else(Vec::new, |inner| inner.methods())
    }
    fn controller(&self) -> brain::environment::Services {
        Some(self.controller.clone())
    }
    async fn call(&self, method: &str, input: Value) -> Result<Value, Error> {
        self.inner
            .as_ref()
            .ok_or_else(denied)?
            .call(method, input)
            .await
    }
    fn cancelled(&self) -> bool {
        self.inner.as_ref().is_some_and(|inner| inner.cancelled())
    }
    async fn closed(&self) {
        if let Some(inner) = &self.inner {
            inner.closed().await;
        }
    }
}
