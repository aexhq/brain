//! The Agentloop worker process: a Unix socket in front of [`WorkerService`].
//!
//! Every connection is served on its own task, so one long activation does not hold up
//! the sessions behind it. What runs at once is bounded inside the service, where the
//! Wasm instances actually live.

#[cfg(unix)]
#[tokio::main]
async fn main() -> Result<(), String> {
    use std::{path::PathBuf, sync::Arc};

    use brain_loophost::{LoopLimits, RUNTIME_SHIM_IMPORTS, WorkerService};
    use tokio::net::UnixListener;

    let socket = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| "usage: brain-loop-worker <socket>".to_owned())?;
    if socket.exists() {
        std::fs::remove_file(&socket).map_err(|error| error.to_string())?;
    }
    let listener = UnixListener::bind(&socket).map_err(|error| error.to_string())?;
    let service = Arc::new(WorkerService::new(
        LoopLimits::default(),
        RUNTIME_SHIM_IMPORTS
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
    )?);

    loop {
        let (mut stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
        let service = service.clone();
        tokio::spawn(async move { service.serve(&mut stream).await });
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("brain-loop-worker supports Linux and other Unix servers only");
    std::process::exit(2);
}
