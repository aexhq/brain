#[tokio::main]
async fn main() -> anyhow::Result<()> {
    brain_component_host::run_worker().await
}
