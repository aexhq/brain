//! Startup registrations and independent worker pools for installed agentloops.

use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use brain::{AgentHarness, Error, TurnServices};
use brain_protocol::{
    AgentloopRegistration, AgentloopSelection, HarnessCommand, SessionConfig, SessionId, TurnInput,
    TurnOutput,
};
use tokio::sync::Semaphore;

use crate::harness::StdioHarness;

struct Worker {
    harness: Arc<StdioHarness>,
    capacity: Semaphore,
    sessions: AtomicUsize,
}

struct Pool {
    workers: Vec<Worker>,
    sessions: Mutex<HashMap<SessionId, usize>>,
}

impl Pool {
    fn worker(&self, session: &SessionId) -> Result<&Worker, Error> {
        let mut sessions = self.sessions.lock().expect("agentloop sessions lock");
        let index = match sessions.get(session) {
            Some(index) => *index,
            None => {
                let index = self
                    .workers
                    .iter()
                    .enumerate()
                    .filter(|(_, worker)| worker.harness.ready())
                    .min_by_key(|(_, worker)| worker.sessions.load(Ordering::Relaxed))
                    .map(|(index, _)| index)
                    .ok_or_else(|| Error::Executor("agentloop has no available workers".into()))?;
                sessions.insert(session.clone(), index);
                self.workers[index].sessions.fetch_add(1, Ordering::Relaxed);
                index
            }
        };
        let worker = &self.workers[index];
        if !worker.harness.ready() {
            return Err(Error::Ambiguous(
                "assigned agentloop worker failed; the session is not reassigned or replayed"
                    .into(),
            ));
        }
        Ok(worker)
    }

    async fn close(&self, session: &SessionId) -> Result<(), Error> {
        let index = self
            .sessions
            .lock()
            .expect("agentloop sessions lock")
            .get(session)
            .copied();
        if let Some(index) = index {
            self.workers[index].harness.close(session).await?;
            self.release(session);
        }
        Ok(())
    }

    fn release(&self, session: &SessionId) {
        if let Some(index) = self
            .sessions
            .lock()
            .expect("agentloop sessions lock")
            .remove(session)
        {
            self.workers[index].sessions.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

pub struct AgentloopRegistry {
    pools: BTreeMap<String, Pool>,
}

impl AgentloopRegistry {
    pub async fn start(
        registrations: &BTreeMap<String, AgentloopRegistration>,
        directory: &Path,
        providers: Arc<brain::model::ProviderRegistry>,
    ) -> Result<Self, Error> {
        if registrations.is_empty() {
            return Err(Error::InvalidState(
                "register at least one agentloop at startup".into(),
            ));
        }
        let mut pools = BTreeMap::new();
        for (name, registration) in registrations {
            brain::validate_agentloop_name(name)?;
            let command = HarnessCommand {
                command: registration.command.clone(),
                args: registration.args.clone(),
                configuration: registration.configuration.clone(),
            };
            let mut workers = Vec::new();
            for _ in 0..registration.workers.get() {
                workers.push(Worker {
                    harness: Arc::new(
                        StdioHarness::start(&command, &directory.join(name), providers.clone())
                            .await?,
                    ),
                    capacity: Semaphore::new(registration.max_concurrency.get()),
                    sessions: AtomicUsize::new(0),
                });
            }
            pools.insert(
                name.clone(),
                Pool {
                    workers,
                    sessions: Mutex::new(HashMap::new()),
                },
            );
        }
        Ok(Self { pools })
    }

    fn pool(&self, name: &str) -> Result<&Pool, Error> {
        self.pools
            .get(name)
            .ok_or_else(|| Error::InvalidState(format!("agentloop {name:?} is not registered")))
    }
}

#[async_trait]
impl AgentHarness for AgentloopRegistry {
    fn validate(&self, selection: &AgentloopSelection) -> Result<(), Error> {
        brain::validate_agentloop_name(&selection.name)?;
        let pool = self.pool(&selection.name)?;
        if !pool.workers.iter().any(|worker| worker.harness.ready()) {
            return Err(Error::Executor(format!(
                "agentloop {:?} has no available workers",
                selection.name
            )));
        }
        Ok(())
    }

    fn ready(&self) -> bool {
        self.pools
            .values()
            .any(|pool| pool.workers.iter().any(|worker| worker.harness.ready()))
    }

    async fn turn(
        &self,
        session: &SessionId,
        sequence: u64,
        config: &SessionConfig,
        input: TurnInput,
        services: Arc<dyn TurnServices>,
    ) -> Result<TurnOutput, Error> {
        let worker = self.pool(&config.agentloop.name)?.worker(session)?;
        let cancelled = async {
            while !services.cancelled() {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        };
        let _permit = tokio::select! {
            biased;
            () = cancelled => return Err(Error::Cancelled("agentloop activation cancelled while waiting for capacity".into())),
            permit = worker.capacity.acquire() => permit.map_err(|_| Error::Executor("agentloop worker capacity closed".into()))?,
        };
        worker
            .harness
            .turn(session, sequence, config, input, services)
            .await
    }

    async fn close(&self, session: &SessionId) -> Result<(), Error> {
        for pool in self.pools.values() {
            pool.close(session).await?;
        }
        Ok(())
    }

    async fn delete(&self, session: &SessionId, config: &SessionConfig) -> Result<(), Error> {
        let pool = self.pool(&config.agentloop.name)?;
        pool.worker(session)?
            .harness
            .delete(session, config)
            .await?;
        pool.release(session);
        Ok(())
    }

    async fn shutdown(&self) {
        let mut tasks = tokio::task::JoinSet::new();
        // Shutdown each process concurrently without restarting or replaying work.
        for pool in self.pools.values() {
            for worker in &pool.workers {
                let harness = worker.harness.clone();
                tasks.spawn(async move {
                    harness.shutdown().await;
                });
            }
        }
        while tasks.join_next().await.is_some() {}
    }
}

#[cfg(test)]
#[path = "agentloops_tests.rs"]
mod tests;
