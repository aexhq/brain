//! The Tool worker process: a local socket in front of [`WorkerService`].
//!
//! Every connection is served on its own task, so one long activation does not hold up
//! the sessions behind it. What runs at once is bounded inside the service, where the
//! Wasm instances actually live.

use std::sync::Arc;

use brain_env::WorkerArgs;
use brain_env_worker::WorkerService;
use clap::Parser as _;

#[tokio::main]
async fn main() -> Result<(), String> {
    let WorkerArgs { socket, limits } = WorkerArgs::parse();
    let mut listener = brain_env::listen(&socket).map_err(|error| error.to_string())?;
    let service = Arc::new(WorkerService::new(limits)?);

    loop {
        let mut stream = listener.accept().await.map_err(|error| error.to_string())?;
        let service = service.clone();
        tokio::spawn(async move { service.serve(&mut stream).await });
    }
}
