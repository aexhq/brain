use std::collections::BTreeMap;

use brain_protocol::{
    Environment, EnvironmentLifecycle, EnvironmentName, EnvironmentRef, EnvironmentState,
    EnvironmentView, SessionConfig, codes,
};

use crate::{Error, SessionStore};

/// A disposable projection. The admitted configuration remains the authority ceiling.
pub fn environments(
    store: &dyn SessionStore,
    config: &SessionConfig,
) -> Result<BTreeMap<EnvironmentName, EnvironmentView>, Error> {
    let mut views: BTreeMap<_, _> = config
        .environments
        .iter()
        .map(|env| {
            (
                env.name.clone(),
                EnvironmentView {
                    reference: EnvironmentRef {
                        name: env.name.clone(),
                        sequence: 1,
                    },
                    template: env.name.clone(),
                    lifecycle: env.lifecycle.unwrap_or(EnvironmentLifecycle::Automatic),
                    state: EnvironmentState::Declared,
                    configuration: env.configuration.clone(),
                    methods: env.methods.clone(),
                    observation: None,
                    observed_at_ms: None,
                    pending_operation: None,
                    availability: None,
                    tools: config
                        .tools
                        .iter()
                        .filter(|tool| tool.placements.contains_key(&env.name))
                        .map(|tool| tool.definition())
                        .collect(),
                },
            )
        })
        .collect();
    let mut operations = BTreeMap::new();
    let mut current_operations = BTreeMap::new();
    let mut after = 0;
    loop {
        let records = store.records_after(after, 1000)?;
        if records.is_empty() {
            break;
        }
        for record in records {
            after = record.sequence;
            match record.kind.as_str() {
                codes::event::ENVIRONMENT_DECLARED => {
                    let mut view: EnvironmentView = serde_json::from_value(record.payload)
                        .map_err(|error| Error::Journal(error.to_string()))?;
                    view.reference.sequence = record.sequence;
                    current_operations.remove(&view.reference.name);
                    views.insert(view.reference.name.clone(), view);
                }
                codes::event::ENVIRONMENT_UPDATED => {
                    if let Some(view) = record.payload["environment"]
                        .as_str()
                        .and_then(|name| views.get_mut(&EnvironmentName::new(name)))
                    {
                        view.configuration = record.payload["configuration"].clone();
                    }
                }
                codes::event::ENVIRONMENT_SETUP_STARTED
                | codes::event::ENVIRONMENT_REPLACE_STARTED
                | codes::event::ENVIRONMENT_TEARDOWN_STARTED
                | codes::event::ENVIRONMENT_DETACH_STARTED => {
                    if let Some(name) = record.payload["environment"].as_str() {
                        let name = EnvironmentName::new(name);
                        if let Some(view) = views.get_mut(&name) {
                            if record.kind == codes::event::ENVIRONMENT_REPLACE_STARTED {
                                view.reference.sequence = record.sequence;
                                view.observation = None;
                                view.observed_at_ms = None;
                                view.availability = None;
                            }
                            view.pending_operation = Some(record.sequence);
                            view.state = if matches!(
                                record.kind.as_str(),
                                codes::event::ENVIRONMENT_SETUP_STARTED
                                    | codes::event::ENVIRONMENT_REPLACE_STARTED
                            ) {
                                EnvironmentState::Starting
                            } else {
                                EnvironmentState::Deleting
                            };
                        }
                        current_operations.insert(name.clone(), record.sequence);
                        operations.insert(record.sequence, (name, record.kind));
                    }
                }
                codes::event::ENVIRONMENT_SETUP_ENDED
                | codes::event::ENVIRONMENT_REPLACE_ENDED
                | codes::event::ENVIRONMENT_REPLACE_FAILED
                | codes::event::ENVIRONMENT_TEARDOWN_ENDED
                | codes::event::ENVIRONMENT_DETACH_ENDED
                | codes::event::ENVIRONMENT_SETUP_FAILED
                | codes::event::ENVIRONMENT_TEARDOWN_FAILED
                | codes::event::ENVIRONMENT_DETACH_FAILED => {
                    if let Some(sequence) = record.payload["sequence"].as_u64()
                        && let Some((name, _)) = operations.get(&sequence)
                        && current_operations.get(name) == Some(&sequence)
                        && let Some(view) = views.get_mut(name)
                        && view.pending_operation == Some(sequence)
                    {
                        view.state = match record.kind.as_str() {
                            codes::event::ENVIRONMENT_SETUP_ENDED
                            | codes::event::ENVIRONMENT_REPLACE_ENDED => EnvironmentState::Ready,
                            codes::event::ENVIRONMENT_TEARDOWN_ENDED => EnvironmentState::Deleted,
                            codes::event::ENVIRONMENT_DETACH_ENDED => EnvironmentState::Detached,
                            _ if record.payload["ambiguous"] == true => EnvironmentState::Unknown,
                            _ => EnvironmentState::Failed,
                        };
                        if view.state != EnvironmentState::Unknown {
                            view.pending_operation = None;
                        }
                    }
                }
                codes::event::ENVIRONMENT_CLOSED => {
                    if let Some(view) = record.payload["environment"]
                        .as_str()
                        .and_then(|name| views.get_mut(&EnvironmentName::new(name)))
                    {
                        view.state = EnvironmentState::Deleted;
                        view.pending_operation = None;
                    }
                }
                codes::event::ENVIRONMENT_OBSERVATION => {
                    if let Some(brain_protocol::EventOrigin::Environment { environment }) =
                        record.origin
                        && let Some(view) = views.get_mut(&environment.name)
                        && view.reference == environment
                    {
                        let observation =
                            serde_json::from_value(record.payload["observation"].clone())
                                .map_err(|error| Error::Journal(error.to_string()))?;
                        if let brain_protocol::EnvironmentObservation::Environment {
                            availability,
                            ..
                        } = &observation
                        {
                            view.availability = Some(*availability);
                        }
                        view.observation = Some(observation);
                        view.observed_at_ms = Some(record.recorded_at_ms);
                    }
                }
                _ => {}
            }
        }
    }
    Ok(views)
}

pub fn descriptor(config: &SessionConfig, view: &EnvironmentView) -> Result<Environment, Error> {
    let mut environment = config
        .environment(&view.template)
        .cloned()
        .ok_or_else(|| Error::Journal("Environment template is missing".into()))?;
    environment.name = view.reference.name.clone();
    environment.configuration = view.configuration.clone();
    Ok(environment)
}
