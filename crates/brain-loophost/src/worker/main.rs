#[cfg(unix)]
#[tokio::main]
async fn main() -> Result<(), String> {
    use brain_loophost::{
        AdmissionEngine, AdmittedAgentloop, LoopLimits, RUNTIME_SHIM_IMPORTS, WorkerRequest,
        WorkerResponse,
    };
    use std::{collections::HashMap, path::PathBuf};
    use tokio::net::UnixListener;

    let socket = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| "usage: brain-loop-worker <socket>".to_owned())?;
    if socket.exists() {
        std::fs::remove_file(&socket).map_err(|error| error.to_string())?;
    }
    let listener = UnixListener::bind(&socket).map_err(|error| error.to_string())?;
    let engine = AdmissionEngine::new(
        LoopLimits::default(),
        RUNTIME_SHIM_IMPORTS
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
    )?;
    let mut admitted: HashMap<String, AdmittedAgentloop> = HashMap::new();
    loop {
        let (mut stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
        let response = match brain_loophost::worker_read(&mut stream).await {
            Ok(WorkerRequest::Ping) => WorkerResponse::Pong,
            Ok(WorkerRequest::Admit { package_json }) => {
                match engine.admit(package_json.as_bytes()) {
                    Ok(component) => {
                        let digest = component.digest.clone();
                        admitted.insert(digest.as_str().to_owned(), component);
                        WorkerResponse::Admitted { digest }
                    }
                    Err(message) => WorkerResponse::Error {
                        code: "admission_failed".into(),
                        message,
                    },
                }
            }
            Ok(WorkerRequest::Activate { digest, input }) => match admitted.get(digest.as_str()) {
                Some(component) => {
                    match component.activate(engine.engine(), engine.limits(), *input) {
                        Ok(output) => WorkerResponse::Activated { output },
                        Err(message) => WorkerResponse::Error {
                            code: "activation_failed".into(),
                            message,
                        },
                    }
                }
                None => WorkerResponse::Error {
                    code: "not_admitted".into(),
                    message: "Agentloop digest is not admitted in this worker".into(),
                },
            },
            Err(message) => WorkerResponse::Error {
                code: "invalid_frame".into(),
                message,
            },
        };
        brain_loophost::worker_write(&mut stream, &response).await?;
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("brain-loop-worker supports Linux and other Unix servers only");
    std::process::exit(2);
}
