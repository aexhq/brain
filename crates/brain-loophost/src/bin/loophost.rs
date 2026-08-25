//! The per-tenant loop-host daemon: loads one guest component and serves activations over the
//! B1 wire. Configuration is by environment, matching the brain binary's convention:
//!
//! - `BRAIN_LOOPHOST_COMPONENT` (required): path to the guest wasm component.
//! - `BRAIN_LOOPHOST_TOKEN` (required): shared secret the brain must present on connect.
//! - `BRAIN_LOOPHOST_LISTEN` (default `127.0.0.1:0`): bind address.
//!
//! The bound address is reported as a single `listening <addr>` line on stdout — the parent
//! reads it to learn an ephemeral port. Logs go to stderr so stdout stays machine-readable.

use std::io::Write;

fn main() -> anyhow::Result<()> {
    let telemetry = brain_observability::install("brain-loophost")?;
    let component = std::env::var("BRAIN_LOOPHOST_COMPONENT").map_err(|_| {
        anyhow::anyhow!("BRAIN_LOOPHOST_COMPONENT is required (path to the guest component)")
    })?;
    let token = std::env::var("BRAIN_LOOPHOST_TOKEN")
        .map_err(|_| anyhow::anyhow!("BRAIN_LOOPHOST_TOKEN is required"))?;
    let listen = std::env::var("BRAIN_LOOPHOST_LISTEN").unwrap_or_else(|_| "127.0.0.1:0".into());

    let engine =
        brain_loophost::WasmLoopEngine::from_component_file(std::path::Path::new(&component))?;
    let result = tokio::runtime::Runtime::new()?.block_on(async move {
        let listener = tokio::net::TcpListener::bind(&listen).await?;
        let addr = listener.local_addr()?;
        println!("listening {addr}");
        std::io::stdout().flush()?;
        tracing::info!(%addr, component, "loop host serving");
        brain_loophost::daemon::serve(listener, engine, token).await
    });
    telemetry.shutdown()?;
    result
}
