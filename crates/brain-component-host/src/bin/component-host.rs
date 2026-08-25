#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let telemetry = brain_observability::install("brain-component-host")?;
    let result = brain_component_host::run_worker().await;
    telemetry.shutdown()?;
    result
}
