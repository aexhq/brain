//! The turns open right now whose loop runs outside this process.
//!
//! An Environment reached over HTTP receives, with each turn, a token minted for that
//! turn and the address of the session's turn routes. The routes resolve here: the
//! activation's own `TurnServices`, the same five services an in-process loop has, so a
//! loop running elsewhere has exactly the authority a loop running here has. The token
//! is hashed in memory, never journaled, and dies with the activation.

use std::{collections::HashMap, sync::Arc, sync::Mutex};

use brain::TurnServices;
use brain_protocol::{
    SessionId, TurnAnswer, TurnCall, TurnCallback, TurnDispatchResult, TurnEmitAck,
};
use sha2::{Digest as _, Sha256};

#[derive(Default)]
pub struct Turns {
    open: Mutex<HashMap<(SessionId, u64), Open>>,
}

struct Open {
    token: [u8; 32],
    services: Arc<dyn TurnServices>,
}

/// Closes the turn's routes when the turn is over, whichever way it ends.
pub struct OpenTurn {
    turns: Arc<Turns>,
    key: (SessionId, u64),
}

impl Drop for OpenTurn {
    fn drop(&mut self) {
        if let Ok(mut open) = self.turns.open.lock() {
            open.remove(&self.key);
        }
    }
}

impl Turns {
    /// Opens the routes for one activation and mints the token that opens them.
    pub fn open(
        self: &Arc<Self>,
        session_id: &SessionId,
        sequence: u64,
        public_url: &str,
        services: Arc<dyn TurnServices>,
    ) -> Result<(TurnCallback, OpenTurn), brain::Error> {
        let token = brain::random_id("btt");
        self.open
            .lock()
            .map_err(|_| brain::Error::Executor("open turn table is poisoned".into()))?
            .insert(
                (session_id.clone(), sequence),
                Open {
                    token: digest(&token),
                    services,
                },
            );
        Ok((
            TurnCallback {
                url: format!(
                    "{}/v1/sessions/{session_id}/turns/{sequence}",
                    public_url.trim_end_matches('/')
                ),
                token,
            },
            OpenTurn {
                turns: self.clone(),
                key: (session_id.clone(), sequence),
            },
        ))
    }

    /// One call on an open turn's routes. A turn that is not open, or a token that is
    /// not its own, is refused the same way: nothing about the turn is revealed.
    pub async fn call(
        &self,
        session_id: &SessionId,
        sequence: u64,
        token: &str,
        call: TurnCall,
    ) -> Result<TurnAnswer, brain::Error> {
        let services = {
            let open = self
                .open
                .lock()
                .map_err(|_| brain::Error::Executor("open turn table is poisoned".into()))?;
            open.get(&(session_id.clone(), sequence))
                .filter(|open| constant_time_equal(&open.token, &digest(token)))
                .map(|open| open.services.clone())
                .ok_or_else(|| brain::Error::NotFound("no such open turn".into()))?
        };
        Ok(match call {
            TurnCall::Events { after } => TurnAnswer::Events(services.events(after).await?),
            TurnCall::Model(request) => TurnAnswer::Model(services.model(request).await?),
            TurnCall::Dispatch(request) => TurnAnswer::Dispatch(TurnDispatchResult {
                results: services.dispatch(request.calls).await?,
            }),
            TurnCall::Emit(request) => TurnAnswer::Emit(TurnEmitAck {
                sequence: services.emit(request.event_type, request.data).await?,
            }),
            TurnCall::Telemetry(telemetry) => {
                services.telemetry(telemetry.record);
                TurnAnswer::Telemetry
            }
        })
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
    use brain_protocol::{ModelRequest, ModelResult, ToolInvocation, ToolResult};

    struct Counting(std::sync::atomic::AtomicUsize);

    #[async_trait::async_trait]
    impl TurnServices for Counting {
        async fn events(&self, after: u64) -> Result<brain_protocol::EventPage, brain::Error> {
            Ok(brain_protocol::EventPage {
                events: Vec::new(),
                next_cursor: after,
            })
        }
        async fn model(&self, _: ModelRequest) -> Result<ModelResult, brain::Error> {
            unreachable!()
        }
        async fn dispatch(&self, _: Vec<ToolInvocation>) -> Result<Vec<ToolResult>, brain::Error> {
            unreachable!()
        }
        async fn emit(&self, _: String, _: serde_json::Value) -> Result<u64, brain::Error> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(9)
        }
        fn telemetry(&self, _: serde_json::Value) {}
        fn cancelled(&self) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn a_turns_routes_open_with_its_token_and_close_with_the_turn() {
        let turns = Arc::new(Turns::default());
        let session = SessionId::new("ses_remote");
        let services = Arc::new(Counting(Default::default()));
        let (callback, open) = turns
            .open(&session, 4, "http://brain.example/", services.clone())
            .unwrap();
        assert_eq!(
            callback.url,
            "http://brain.example/v1/sessions/ses_remote/turns/4"
        );
        let emit = || {
            TurnCall::Emit(brain_protocol::TurnEmitRequest {
                event_type: "note".into(),
                data: serde_json::json!({}),
            })
        };
        assert!(matches!(
            turns
                .call(&session, 4, &callback.token, emit())
                .await
                .unwrap(),
            TurnAnswer::Emit(TurnEmitAck { sequence: 9 })
        ));
        assert!(turns.call(&session, 4, "wrong", emit()).await.is_err());
        assert!(
            turns
                .call(&session, 5, &callback.token, emit())
                .await
                .is_err()
        );
        drop(open);
        assert!(
            turns
                .call(&session, 4, &callback.token, emit())
                .await
                .is_err(),
            "a closed turn answers nothing"
        );
        assert_eq!(services.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
