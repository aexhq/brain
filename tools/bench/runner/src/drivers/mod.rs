//! One driver per subject that the runner measures itself.
//!
//! Brain lands first because it is the reference: until Brain's own numbers are current
//! on the rebuilt harness, there is nothing for another subject's number to be compared
//! against. OpenFang, Letta and LangGraph Server follow, each mapping its own API onto
//! `Driver` and declaring what its timing means in its manifest.
//!
//! Adding one is a file here, a line in `for_subject`, and a manifest under `subjects/`.
//! Nothing outside `tools/bench` changes: the runner depends on no brain crate and talks
//! to every subject, Brain included, over its public HTTP surface.

use std::{path::PathBuf, sync::Arc};

use anyhow::Result;

use crate::{driver::Driver, fixtures::Fixture};

pub mod agentscope;
pub mod agno_agentos;
pub mod awaken;
pub mod brain;
pub mod daytona;
pub mod e2b;
pub mod e2b_selfhosted;
pub mod firecracker;
pub mod framework;
pub mod managed_agents;
pub mod mastra;
pub mod modal;
pub mod openclaw;
pub mod restate;
pub mod temporal;
pub mod voltagent;
pub mod openfang;
pub mod zeroclaw;
pub mod langgraph_server;
pub mod letta;

/// What any driver may need to be built. Passing a context rather than a growing argument
/// list means adding a driver that needs something new does not churn every other one.
pub struct Bench {
    /// Where the subject is listening.
    pub base_url: String,
    /// The compiled Agentloop package. Brain's alone; another subject's equivalent
    /// belongs beside it rather than in place of it.
    pub agentloop_package: PathBuf,
    /// The subject's process, when the runner started it rather than being pointed at one.
    pub pid: Option<u32>,
    /// The echo environment every subject's tools are bound to.
    pub environment: Arc<Fixture>,
    /// Where the scripted provider is listening. A subject that is *told* its model gets
    /// this through its launch environment; one that registers providers through its own
    /// API needs the driver to hand it over.
    pub model_base_url: String,
}

/// Whether this subject's driver supplies its own endpoint, so the runner neither starts
/// it nor needs to be told where it is. True for hosted services with a published API
/// host; false for anything the runner launches or is pointed at.
pub fn carries_its_own_endpoint(subject: &str) -> bool {
    matches!(
        subject,
        "daytona" | "e2b-cloud" | "modal" | "claude-managed-agents"
    )
}

/// The driver for `subject`, or `None` when nobody has written one yet.
///
/// A missing driver is recorded as a skip with that reason. It is never substituted with
/// another subject's driver: every subject speaks a different API, so driving one with
/// another's client produces numbers that look real and mean nothing.
pub fn for_subject(subject: &str, bench: &Bench) -> Result<Option<Box<dyn Driver>>> {
    match subject {
        "brain" => Ok(Some(Box::new(brain::BrainDriver::new(
            &bench.base_url,
            bench.agentloop_package.clone(),
            bench.pid,
            Arc::clone(&bench.environment),
        )?))),
        "agentscope-runtime" => Ok(Some(Box::new(agentscope::AgentScopeDriver::new(
            &bench.base_url,
            bench.pid,
        )?))),
        "awaken" => Ok(Some(Box::new(awaken::AwakenDriver::new(
            &bench.base_url,
            bench.pid,
        )?))),
        "agno-agentos" => Ok(Some(Box::new(agno_agentos::AgnoAgentOsDriver::new(
            &bench.base_url,
            bench.pid,
        )?))),
        "langgraph-server" => Ok(Some(Box::new(
            langgraph_server::LangGraphServerDriver::new(&bench.base_url, bench.pid)?,
        ))),
        "letta" => Ok(Some(Box::new(letta::LettaDriver::new(
            &bench.base_url,
            &bench.model_base_url,
            bench.pid,
        )?))),
        "daytona" => Ok(Some(Box::new(daytona::DaytonaDriver::new()?))),
        "e2b-cloud" => Ok(Some(Box::new(e2b::E2bDriver::new()?))),
        // Their OSS build on our instance, never merged with the cloud rows above.
        "e2b-selfhosted" => Ok(Some(Box::new(e2b_selfhosted::E2bSelfhostedDriver::new(
            &bench.base_url,
            bench.pid,
        )?))),
        // Spawns its own VMMs and talks to each over its own Unix socket, so it needs no
        // endpoint from the runner.
        "firecracker" => Ok(Some(Box::new(firecracker::FirecrackerDriver::new()?))),
        // Every framework subject goes through the harness we wrote for it.
        "langgraph" | "crewai" | "autogen" | "microsoft-agent-framework" => Ok(Some(Box::new(
            framework::FrameworkDriver::new(&bench.base_url, bench.pid)?,
        ))),
        "openfang" => Ok(Some(Box::new(openfang::OpenFangDriver::new(
            &bench.base_url,
            bench.pid,
        )?))),
        "claude-managed-agents" => Ok(Some(Box::new(
            managed_agents::ManagedAgentsDriver::new()?,
        ))),
        "modal" => Ok(Some(Box::new(modal::ModalDriver::new()?))),
        "zeroclaw" => Ok(Some(Box::new(zeroclaw::ZeroclawDriver::new(
            &bench.base_url,
            bench.pid,
        )?))),
        "mastra" => Ok(Some(Box::new(mastra::MastraDriver::new(
            &bench.base_url,
            bench.pid,
        )?))),
        "temporal" => Ok(Some(Box::new(temporal::TemporalDriver::new(
            &bench.base_url,
            bench.pid,
        )?))),
        "restate" => Ok(Some(Box::new(restate::RestateDriver::new(
            &bench.base_url,
            bench.pid,
        )?))),
        "voltagent" => Ok(Some(Box::new(voltagent::VoltAgentDriver::new(
            &bench.base_url,
            bench.pid,
        )?))),
        "openclaw" => Ok(Some(Box::new(openclaw::OpenclawDriver::new(
            &bench.base_url,
            bench.pid,
        )?))),
        _ => Ok(None),
    }
}
