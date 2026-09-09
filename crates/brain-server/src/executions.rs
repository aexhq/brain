//! Invocation-scoped callback grants for remote Environment execution.

use std::{collections::HashMap, sync::Arc, sync::Mutex};

use brain::environment::ExecutionServices;
use brain_protocol::{ExecutionCall, ExecutionCallback, SessionId};
use sha2::{Digest as _, Sha256};

#[derive(Default)]
pub struct Executions {
    open: Mutex<HashMap<(SessionId, u64), Open>>,
}

struct Open {
    token: [u8; 32],
    services: Arc<dyn ExecutionServices>,
}

/// Revokes invocation credentials on completion, cancellation, or failure.
pub struct OpenExecution {
    executions: Arc<Executions>,
    key: (SessionId, u64),
}

impl Drop for OpenExecution {
    fn drop(&mut self) {
        if let Ok(mut open) = self.executions.open.lock() {
            open.remove(&self.key);
        }
    }
}

impl Executions {
    /// Grants only the supplied services for this invocation.
    pub fn open(
        self: &Arc<Self>,
        session_id: &SessionId,
        sequence: u64,
        public_url: &str,
        services: Arc<dyn ExecutionServices>,
    ) -> Result<(ExecutionCallback, OpenExecution), brain::Error> {
        let token = brain::random_id("bex");
        self.open
            .lock()
            .map_err(|_| brain::Error::Executor("open execution table is poisoned".into()))?
            .insert(
                (session_id.clone(), sequence),
                Open {
                    token: digest(&token),
                    services: services.clone(),
                },
            );
        Ok((
            ExecutionCallback {
                url: format!(
                    "{}/v1/sessions/{session_id}/executions/{sequence}/call",
                    public_url.trim_end_matches('/')
                ),
                token,
                methods: services
                    .methods()
                    .iter()
                    .map(|method| (*method).to_owned())
                    .collect(),
            },
            OpenExecution {
                executions: self.clone(),
                key: (session_id.clone(), sequence),
            },
        ))
    }

    /// Unknown invocations and invalid credentials receive the same response.
    pub async fn call(
        &self,
        session_id: &SessionId,
        sequence: u64,
        token: &str,
        call: ExecutionCall,
    ) -> Result<serde_json::Value, brain::Error> {
        let services = {
            let open = self
                .open
                .lock()
                .map_err(|_| brain::Error::Executor("open execution table is poisoned".into()))?;
            open.get(&(session_id.clone(), sequence))
                .filter(|open| constant_time_equal(&open.token, &digest(token)))
                .map(|open| open.services.clone())
                .ok_or_else(|| brain::Error::NotFound("no such open execution".into()))?
        };
        if !services.methods().contains(&call.method.as_str()) {
            return Err(brain::Error::InvalidState(
                "execution service is not granted".into(),
            ));
        }
        if services.cancelled() {
            return Err(brain::Error::Cancelled("execution cancelled".into()));
        }
        services.call(&call.method, call.input).await
    }
}

fn digest(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

fn constant_time_equal(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Default)]
    struct Counting {
        calls: AtomicUsize,
        cancelled: AtomicBool,
    }

    #[async_trait::async_trait]
    impl ExecutionServices for Counting {
        fn methods(&self) -> &'static [&'static str] {
            &["emit", "telemetry"]
        }
        async fn call(
            &self,
            method: &str,
            _: serde_json::Value,
        ) -> Result<serde_json::Value, brain::Error> {
            assert_eq!(method, "emit");
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(9.into())
        }
        fn cancelled(&self) -> bool {
            self.cancelled.load(Ordering::SeqCst)
        }
    }

    #[tokio::test]
    async fn callbacks_require_the_invocation_token_and_granted_method() {
        let executions = Arc::new(Executions::default());
        let session = SessionId::new("ses_remote");
        let services = Arc::new(Counting::default());
        let (callback, open) = executions
            .open(&session, 4, "http://brain.example/", services.clone())
            .unwrap();
        assert_eq!(
            callback.url,
            "http://brain.example/v1/sessions/ses_remote/executions/4/call"
        );
        assert_eq!(callback.methods, ["emit", "telemetry"]);
        let call = |method: &str| ExecutionCall {
            method: method.into(),
            input: serde_json::json!({}),
        };
        assert_eq!(
            executions
                .call(&session, 4, &callback.token, call("emit"))
                .await
                .unwrap(),
            9
        );
        assert!(
            executions
                .call(&session, 4, "wrong", call("emit"))
                .await
                .is_err()
        );
        assert!(
            executions
                .call(&session, 5, &callback.token, call("emit"))
                .await
                .is_err()
        );
        assert!(
            executions
                .call(
                    &SessionId::new("ses_other"),
                    4,
                    &callback.token,
                    call("emit")
                )
                .await
                .is_err()
        );
        for method in [
            "model",
            "dispatch",
            "events",
            "set_transcript",
            "kv_put",
            "kv_read",
            "kv_delete",
        ] {
            assert!(
                executions
                    .call(&session, 4, &callback.token, call(method))
                    .await
                    .is_err()
            );
        }
        services.cancelled.store(true, Ordering::SeqCst);
        assert!(
            executions
                .call(&session, 4, &callback.token, call("emit"))
                .await
                .is_err()
        );
        services.cancelled.store(false, Ordering::SeqCst);
        drop(open);
        assert!(
            executions
                .call(&session, 4, &callback.token, call("emit"))
                .await
                .is_err()
        );
        assert_eq!(services.calls.load(Ordering::SeqCst), 1);
    }
}
