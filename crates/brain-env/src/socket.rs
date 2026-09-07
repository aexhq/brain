//! The local socket between the server and its worker processes.
//!
//! Both sides address the socket by a filesystem path. On Unix that is a Unix domain socket
//! at the path; on Windows it is a named pipe whose name is derived from the path, since a
//! pipe cannot live inside a directory. Either way the stream is a byte stream that
//! [`crate::wire`] frames.

use std::io;
use std::path::Path;

#[cfg(unix)]
pub(crate) type Stream = tokio::net::UnixStream;
#[cfg(windows)]
pub(crate) type Stream = tokio::net::windows::named_pipe::NamedPipeClient;

#[cfg(unix)]
pub(crate) async fn connect(path: &Path) -> io::Result<Stream> {
    tokio::net::UnixStream::connect(path).await
}

/// A pipe server instance answers one client; between one client's connection and the next
/// instance's creation a connect attempt sees `ERROR_PIPE_BUSY`, and the caller retries as
/// Windows documents.
#[cfg(windows)]
pub(crate) async fn connect(path: &Path) -> io::Result<Stream> {
    use tokio::net::windows::named_pipe::ClientOptions;

    const ERROR_PIPE_BUSY: i32 = 231;
    let name = pipe_name(path);
    loop {
        match ClientOptions::new().open(&name) {
            Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            outcome => return outcome,
        }
    }
}

/// Serves the socket at the path, replacing whatever a previous worker left there.
pub fn listen(path: &Path) -> io::Result<Listener> {
    unlink(path)?;
    Listener::bind(path)
}

/// Removes whatever a previous worker left at the path so a new one can bind it.
pub(crate) fn unlink(path: &Path) -> io::Result<()> {
    if cfg!(unix) && path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(unix)]
pub struct Listener(tokio::net::UnixListener);

#[cfg(unix)]
impl Listener {
    fn bind(path: &Path) -> io::Result<Self> {
        tokio::net::UnixListener::bind(path).map(Self)
    }

    pub async fn accept(&mut self) -> io::Result<tokio::net::UnixStream> {
        self.0.accept().await.map(|(stream, _)| stream)
    }
}

/// Named pipes have one server instance per client, so accepting means handing out the
/// instance a client connected to and creating the next one.
#[cfg(windows)]
pub struct Listener {
    name: String,
    next: tokio::net::windows::named_pipe::NamedPipeServer,
}

#[cfg(windows)]
impl Listener {
    fn bind(path: &Path) -> io::Result<Self> {
        use tokio::net::windows::named_pipe::ServerOptions;

        let name = pipe_name(path);
        let next = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&name)?;
        Ok(Self { name, next })
    }

    pub async fn accept(&mut self) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
        use tokio::net::windows::named_pipe::ServerOptions;

        self.next.connect().await?;
        let following = ServerOptions::new().create(&self.name)?;
        Ok(std::mem::replace(&mut self.next, following))
    }
}

/// A pipe name is one flat namespace with a length limit, so the path is hashed into it.
#[cfg(windows)]
fn pipe_name(path: &Path) -> String {
    use sha2::{Digest as _, Sha256};

    let digest = Sha256::digest(path.to_string_lossy().as_bytes());
    let hex: String = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!(r"\\.\pipe\brain-env-{hex}")
}
