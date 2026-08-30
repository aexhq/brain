//! What every session-kernel subject must be able to do to be measured.
//!
//! The trait is deliberately small. Each subject's API is different — Brain admits an
//! agentloop then creates a session, Letta creates an agent, LangGraph creates a thread —
//! and the driver's job is to map that onto the same four moments, then report what its
//! own timing means so the definition travels with the number.

use anyhow::Result;
use async_trait::async_trait;

/// A live unit of work. For a session kernel this is a session; a sandbox driver would
/// carry a sandbox id.
#[derive(Clone, Debug)]
pub struct Unit {
    pub id: String,
}

#[async_trait]
pub trait Driver: Send + Sync {
    /// Anything that must happen once before any probe: uploading an agentloop, warming
    /// a template, creating a project. Never timed, because it is not per-session work
    /// and every subject front-loads a different amount of it.
    async fn prepare(&mut self) -> Result<()> {
        Ok(())
    }

    /// Create a unit and return when it will accept work.
    async fn create(&self) -> Result<Unit>;

    /// Submit work, and return the milliseconds until the first useful output byte
    /// reaches this process.
    async fn ttfb_ms(&self, unit: &Unit) -> Result<f64>;

    /// Submit work, and return the milliseconds until it is complete.
    async fn round_trip_ms(&self, unit: &Unit) -> Result<f64>;

    /// Submit work that provokes a tool call, and return the milliseconds until the bound
    /// environment received it.
    ///
    /// Measured at the environment, not from the event log: the benchmark owns the echo
    /// environment, so it can timestamp arrival directly. Event timestamps would be the
    /// alternative, and their millisecond granularity is too coarse for a number expected
    /// to land under a millisecond.
    async fn tool_dispatch_ms(&self, _unit: &Unit) -> Result<f64> {
        anyhow::bail!("this subject has no tool-dispatch path wired")
    }

    /// Destroy the unit. Must not return until the subject has actually released it, or
    /// a reclaim measurement taken afterwards is meaningless.
    async fn destroy(&self, unit: &Unit) -> Result<()>;

    /// The pid whose process tree holds the subject's memory, when the runner started it.
    /// `None` for a hosted service, which is why those get a proxy resident number.
    fn pid(&self) -> Option<u32> {
        None
    }

    /// How many timed turns this driver has submitted.
    ///
    /// Required rather than defaulted, because the runner checks it against the calls the
    /// scripted provider actually served and a default of zero would make that check pass
    /// for a subject that never reached a model. A turn that did not happen must not be
    /// able to arrive as a latency sample, and the way to guarantee that for a driver
    /// nobody has written yet is to make its author say the number.
    fn turns_requested(&self) -> u64;
}
