//! ZeroClaw, driven through its gateway's chat WebSocket.
//!
//! ZeroClaw is a Rust daemon with a SQLite session store behind an HTTP + WebSocket
//! gateway, which makes it the second-closest architectural comparison to Brain after
//! OpenFang. It is also, by its own documentation, a single-operator tool: one
//! installation runs one configured agent, and "multi-channel" means that one agent is
//! reachable from many places rather than that many sessions run at once. So this driver
//! answers `create`, `ttfb` and `round_trip` and nothing else — see the manifest.
//!
//! **Why the WebSocket and not an HTTP route.** The gateway's HTTP surface cannot run a
//! turn. `POST /api/sessions/{id}/messages` only appends a message to the session log for
//! the dashboard — it never reaches the agent — and `POST /webhook` does run a turn but is
//! rate limited per client key, has no streamed output to time a first byte against, and
//! has no session-create call. `GET /ws/chat` is the surface that has all three: the
//! upgrade creates the session, `chunk` frames carry assistant output, and a `done` frame
//! closes the turn. It is what ZeroClaw's own dashboard uses.
//!
//! The frames, from `crates/zeroclaw-gateway/src/ws.rs` at v0.8.4:
//! `session_start` → (client `connect`) → `connected` → (client `message`) → `agent_start`
//! → `chunk`* → `done`.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::driver::{Driver, Unit};

/// The agent alias the launcher configures under `[agents.<alias>]`. ZeroClaw has no
/// default agent — `/ws/chat` rejects the upgrade outright without `?agent=` — so the
/// driver and the launcher have to agree on one name.
const AGENT_ALIAS: &str = "bench";

/// What every timed turn says. Fixed, because prompt length is a real input to turn cost
/// and a subject given a different prompt than another is not a comparison.
const PROMPT: &str = "benchmark";

pub struct ZeroclawDriver {
    http: reqwest::Client,
    base_url: String,
    host: String,
    port: u16,
    pid: Option<u32>,
    turns_requested: AtomicU64,
    /// The live chat sockets, by session id. A `Driver` takes `&self`, and a turn has to
    /// write to the same socket the session was created on: reconnecting per turn would
    /// fold a TCP connect and a WebSocket upgrade into every latency sample.
    sessions: Mutex<HashMap<String, Arc<Mutex<ws::WsConn>>>>,
}

impl ZeroclawDriver {
    pub fn new(base_url: impl Into<String>, pid: Option<u32>) -> Result<Self> {
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        let (host, port) = split_host_port(&base_url)
            .with_context(|| format!("reading a host and port out of {base_url}"))?;
        Ok(Self {
            http: reqwest::Client::builder()
                .no_proxy()
                // Matched to Brain's, so neither subject's number is its client's pool.
                .pool_max_idle_per_host(512)
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            base_url,
            host,
            port,
            pid,
            turns_requested: AtomicU64::new(0),
            sessions: Mutex::new(HashMap::new()),
        })
    }

    async fn socket(&self, unit: &Unit) -> Result<Arc<Mutex<ws::WsConn>>> {
        self.sessions
            .lock()
            .await
            .get(&unit.id)
            .cloned()
            .with_context(|| format!("no live chat socket for session {}", unit.id))
    }

    /// One turn. Returns the time to the first assistant `chunk` when `to_first_byte` is
    /// set, and the time to `done` otherwise — but always reads through to `done`, so the
    /// next turn on this socket starts on a clean stream and so a sample can never be
    /// reported for a turn that did not finish.
    async fn turn(&self, unit: &Unit, to_first_byte: bool) -> Result<f64> {
        let socket = self.socket(unit).await?;
        let mut socket = socket.lock().await;
        self.turns_requested.fetch_add(1, Ordering::Relaxed);
        let started = Instant::now();
        socket
            .send_text(&json!({ "type": "message", "content": PROMPT }).to_string())
            .await
            .context("sending a message frame")?;

        let mut first_byte_ms: Option<f64> = None;
        loop {
            let frame = socket.next_text().await.context("reading a chat frame")?;
            let frame: Value = serde_json::from_str(&frame)
                .with_context(|| format!("parsing a chat frame: {frame}"))?;
            match frame.get("type").and_then(Value::as_str) {
                // First model output, not first frame: `session_start`, `connected` and
                // `agent_start` all arrive earlier, and timing those would flatter
                // whichever subject emits more lifecycle chatter.
                Some("chunk") if first_byte_ms.is_none() => {
                    first_byte_ms = Some(started.elapsed().as_secs_f64() * 1_000.0);
                }
                Some("done") => {
                    let complete_ms = started.elapsed().as_secs_f64() * 1_000.0;
                    // A turn that produced no reply did not happen, whatever frame said
                    // so. Same check Brain's driver makes, applied to the rival.
                    let reply = frame
                        .get("full_response")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    anyhow::ensure!(!reply.trim().is_empty(), "the turn produced no reply: {frame}");
                    return if to_first_byte {
                        first_byte_ms.context(
                            "the turn completed without ever emitting an assistant chunk, so \
                             there is no first byte to time",
                        )
                    } else {
                        Ok(complete_ms)
                    };
                }
                Some("error") => anyhow::bail!("zeroclaw answered with an error frame: {frame}"),
                _ => {}
            }
        }
    }
}

#[async_trait]
impl Driver for ZeroclawDriver {
    async fn create(&self) -> Result<Unit> {
        let id = format!("bench-{}", unique());
        let path = format!("/ws/chat?agent={AGENT_ALIAS}&session_id={id}");
        let mut socket = ws::WsConn::connect(&self.host, self.port, &path)
            .await
            .with_context(|| format!("opening {}{path}", self.base_url))?;

        // The gateway sends `session_start` on upgrade, then waits for the client's
        // `connect` frame before it builds the agent. Both are part of getting a session
        // that will accept a message, so both are inside the timed window.
        wait_for(&mut socket, "session_start").await?;
        socket
            .send_text(&json!({ "type": "connect", "session_id": id }).to_string())
            .await
            .context("sending the connect frame")?;
        wait_for(&mut socket, "connected").await?;

        self.sessions
            .lock()
            .await
            .insert(id.clone(), Arc::new(Mutex::new(socket)));
        Ok(Unit { id })
    }

    async fn ttfb_ms(&self, unit: &Unit) -> Result<f64> {
        self.turn(unit, true).await
    }

    async fn round_trip_ms(&self, unit: &Unit) -> Result<f64> {
        self.turn(unit, false).await
    }

    async fn destroy(&self, unit: &Unit) -> Result<()> {
        if let Some(socket) = self.sessions.lock().await.remove(&unit.id) {
            socket.lock().await.close().await;
        }
        let response = self
            .http
            .delete(format!("{}/api/sessions/{}", self.base_url, unit.id))
            .send()
            .await?;
        // A gateway built without session persistence answers 404 here, and a session that
        // was never written is already released. Anything else is a real failure and its
        // body is the only thing that says which.
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }
        ok(response, "deleting a session").await?;
        Ok(())
    }

    fn pid(&self) -> Option<u32> {
        self.pid
    }

    fn turns_requested(&self) -> u64 {
        self.turns_requested.load(Ordering::Relaxed)
    }
}

/// Reads frames until one carries `type == expected`, failing loudly on an error frame.
///
/// Lifecycle frames and broadcast events (cron results, heartbeats) share this socket, so
/// a client that assumed the next frame was the one it asked for would break on the first
/// scheduled job.
async fn wait_for(socket: &mut ws::WsConn, expected: &str) -> Result<()> {
    loop {
        let frame = socket
            .next_text()
            .await
            .with_context(|| format!("waiting for a {expected} frame"))?;
        let frame: Value = serde_json::from_str(&frame)
            .with_context(|| format!("parsing a chat frame: {frame}"))?;
        match frame.get("type").and_then(Value::as_str) {
            Some(kind) if kind == expected => return Ok(()),
            Some("error") => anyhow::bail!("zeroclaw answered with an error frame: {frame}"),
            _ => {}
        }
    }
}

async fn ok(response: reqwest::Response, doing: &str) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response
        .text()
        .await
        .unwrap_or_else(|error| format!("<body unreadable: {error}>"));
    anyhow::bail!("{doing}: {status}: {body}")
}

/// Splits `http://127.0.0.1:42617` into its host and port.
fn split_host_port(base_url: &str) -> Option<(String, u16)> {
    let authority = base_url
        .split_once("://")
        .map_or(base_url, |(_, rest)| rest)
        .split('/')
        .next()?;
    let (host, port) = authority.rsplit_once(':')?;
    Some((host.to_owned(), port.parse().ok()?))
}

fn unique() -> String {
    format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default()
    )
}

/// A WebSocket client, small enough to read in one sitting.
///
/// Two of the subjects in this survey — ZeroClaw and OpenClaw — put the operations this
/// benchmark has to time behind a WebSocket rather than behind HTTP, so the runner needs a
/// client for one. It is written here rather than pulled in because the runner's whole
/// dependency set is deliberately small, and because what is needed is the plaintext
/// loopback subset of RFC 6455: masked text frames out, unmasked text frames in, ping
/// answered, close observed. There is no TLS path and no permessage-deflate: both subjects
/// are started by the runner on 127.0.0.1, and a compressed frame would put a decompressor
/// inside a latency measurement.
pub(crate) mod ws {
    use anyhow::{Context, Result};
    use tokio::{
        io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
        net::{
            TcpStream,
            tcp::{OwnedReadHalf, OwnedWriteHalf},
        },
    };

    const OPCODE_CONTINUATION: u8 = 0x0;
    const OPCODE_TEXT: u8 = 0x1;
    const OPCODE_BINARY: u8 = 0x2;
    const OPCODE_CLOSE: u8 = 0x8;
    const OPCODE_PING: u8 = 0x9;
    const OPCODE_PONG: u8 = 0xa;

    /// Refuses a frame larger than this rather than allocating whatever a peer claims.
    /// Turn frames here are kilobytes; the cap only ever fires on a corrupt stream.
    const MAX_FRAME_BYTES: u64 = 32 * 1024 * 1024;

    pub struct WsConn {
        reader: BufReader<OwnedReadHalf>,
        writer: OwnedWriteHalf,
        random: Random,
    }

    impl WsConn {
        /// Opens a connection and completes the upgrade, or fails with the status line and
        /// body the server answered with — which is where "unknown agent", "missing agent
        /// parameter" and "unauthorized" all arrive.
        pub async fn connect(host: &str, port: u16, path: &str) -> Result<Self> {
            let stream = TcpStream::connect((host, port))
                .await
                .with_context(|| format!("connecting to {host}:{port}"))?;
            // Nagle would batch a small frame behind the previous one's ack, which on
            // loopback is milliseconds added to exactly the number being measured.
            stream.set_nodelay(true)?;
            let (reader, writer) = stream.into_split();
            let mut connection = Self {
                reader: BufReader::new(reader),
                writer,
                random: Random::new(),
            };

            let key = base64(&connection.random.bytes::<16>());
            let request = format!(
                "GET {path} HTTP/1.1\r\n\
                 Host: {host}:{port}\r\n\
                 Upgrade: websocket\r\n\
                 Connection: Upgrade\r\n\
                 Sec-WebSocket-Key: {key}\r\n\
                 Sec-WebSocket-Version: 13\r\n\
                 \r\n"
            );
            connection.writer.write_all(request.as_bytes()).await?;
            connection.writer.flush().await?;

            let mut status = String::new();
            connection.reader.read_line(&mut status).await?;
            let mut headers = String::new();
            loop {
                let mut line = String::new();
                let read = connection.reader.read_line(&mut line).await?;
                if read == 0 || line == "\r\n" || line == "\n" {
                    break;
                }
                headers.push_str(&line);
            }
            anyhow::ensure!(
                status.contains(" 101 "),
                "the upgrade was refused: {}{headers}",
                status.trim_end()
            );
            Ok(connection)
        }

        pub async fn send_text(&mut self, text: &str) -> Result<()> {
            self.write_frame(OPCODE_TEXT, text.as_bytes()).await
        }

        /// The next text payload, with control frames handled on the way.
        pub async fn next_text(&mut self) -> Result<String> {
            let mut message = Vec::new();
            let mut kind: Option<u8> = None;
            loop {
                let frame = self.read_frame().await?;
                match frame.opcode {
                    OPCODE_TEXT | OPCODE_BINARY => {
                        kind = Some(frame.opcode);
                        message = frame.payload;
                    }
                    OPCODE_CONTINUATION => message.extend_from_slice(&frame.payload),
                    OPCODE_PING => {
                        self.write_frame(OPCODE_PONG, &frame.payload).await?;
                        continue;
                    }
                    OPCODE_PONG => continue,
                    OPCODE_CLOSE => {
                        anyhow::bail!(
                            "the subject closed the connection: {}",
                            close_reason(&frame.payload)
                        )
                    }
                    other => anyhow::bail!("unexpected websocket opcode {other:#x}"),
                }
                if frame.fin {
                    anyhow::ensure!(
                        kind == Some(OPCODE_TEXT),
                        "expected a text frame, got opcode {:?}",
                        kind
                    );
                    return String::from_utf8(message).context("a text frame was not UTF-8");
                }
            }
        }

        /// Best-effort close. The subject is about to be torn down either way, so a peer
        /// that has already gone is not an error worth surfacing over the probe's own.
        pub async fn close(&mut self) {
            let _ = self.write_frame(OPCODE_CLOSE, &1000_u16.to_be_bytes()).await;
            let _ = self.writer.shutdown().await;
        }

        async fn write_frame(&mut self, opcode: u8, payload: &[u8]) -> Result<()> {
            let mut frame = Vec::with_capacity(payload.len() + 14);
            frame.push(0x80 | opcode);
            let length = payload.len();
            if length < 126 {
                frame.push(0x80 | length as u8);
            } else if let Ok(length) = u16::try_from(length) {
                frame.push(0x80 | 126);
                frame.extend_from_slice(&length.to_be_bytes());
            } else {
                frame.push(0x80 | 127);
                frame.extend_from_slice(&(length as u64).to_be_bytes());
            }
            // Client frames must be masked. This is not a confidentiality measure — the
            // mask travels with the frame — it exists so an intermediary cannot be tricked
            // into treating frame bytes as a request, which is why any random-looking key
            // satisfies it.
            let mask = self.random.bytes::<4>();
            frame.extend_from_slice(&mask);
            frame.extend(
                payload
                    .iter()
                    .zip(mask.iter().cycle())
                    .map(|(byte, key)| byte ^ key),
            );
            self.writer.write_all(&frame).await?;
            self.writer.flush().await?;
            Ok(())
        }

        async fn read_frame(&mut self) -> Result<Frame> {
            let mut header = [0_u8; 2];
            self.reader.read_exact(&mut header).await?;
            let fin = header[0] & 0x80 != 0;
            let opcode = header[0] & 0x0f;
            let masked = header[1] & 0x80 != 0;
            let length = match header[1] & 0x7f {
                126 => {
                    let mut extended = [0_u8; 2];
                    self.reader.read_exact(&mut extended).await?;
                    u64::from(u16::from_be_bytes(extended))
                }
                127 => {
                    let mut extended = [0_u8; 8];
                    self.reader.read_exact(&mut extended).await?;
                    u64::from_be_bytes(extended)
                }
                short => u64::from(short),
            };
            anyhow::ensure!(
                length <= MAX_FRAME_BYTES,
                "the subject announced a {length}-byte frame, past this client's cap"
            );
            let mask = if masked {
                let mut mask = [0_u8; 4];
                self.reader.read_exact(&mut mask).await?;
                Some(mask)
            } else {
                None
            };
            let mut payload = vec![0_u8; length as usize];
            self.reader.read_exact(&mut payload).await?;
            if let Some(mask) = mask {
                for (index, byte) in payload.iter_mut().enumerate() {
                    *byte ^= mask[index % 4];
                }
            }
            Ok(Frame {
                fin,
                opcode,
                payload,
            })
        }
    }

    struct Frame {
        fin: bool,
        opcode: u8,
        payload: Vec<u8>,
    }

    fn close_reason(payload: &[u8]) -> String {
        if payload.len() < 2 {
            return "no reason given".to_owned();
        }
        let code = u16::from_be_bytes([payload[0], payload[1]]);
        format!("{code} {}", String::from_utf8_lossy(&payload[2..]))
    }

    /// Enough randomness for a mask and a handshake key. Neither is a secret: the mask is
    /// sent in the clear beside the bytes it masks, and the key only proves to the server
    /// that the client is speaking WebSocket rather than replaying a cached response.
    struct Random(u64);

    impl Random {
        fn new() -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos() as u64)
                .unwrap_or(0x2545_f491_4f6c_dd1d);
            Self(nanos | 1)
        }

        fn next(&mut self) -> u64 {
            // xorshift64*, which is four instructions and plenty for this.
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn bytes<const N: usize>(&mut self) -> [u8; N] {
            let mut out = [0_u8; N];
            for chunk in out.chunks_mut(8) {
                let source = self.next().to_le_bytes();
                let take = chunk.len();
                chunk.copy_from_slice(&source[..take]);
            }
            out
        }
    }

    fn base64(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let mut block = [0_u8; 3];
            block[..chunk.len()].copy_from_slice(chunk);
            let packed = u32::from(block[0]) << 16 | u32::from(block[1]) << 8 | u32::from(block[2]);
            for index in 0..4 {
                if index <= chunk.len() {
                    let shift = 18 - index * 6;
                    out.push(ALPHABET[((packed >> shift) & 0x3f) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn base64_matches_the_rfc_vectors() {
            assert_eq!(base64(b""), "");
            assert_eq!(base64(b"f"), "Zg==");
            assert_eq!(base64(b"fo"), "Zm8=");
            assert_eq!(base64(b"foo"), "Zm9v");
            assert_eq!(base64(b"foob"), "Zm9vYg==");
            assert_eq!(base64(b"fooba"), "Zm9vYmE=");
            assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        }

        #[test]
        fn a_handshake_key_is_sixteen_bytes_of_base64() {
            let key = base64(&Random::new().bytes::<16>());
            assert_eq!(key.len(), 24, "16 bytes encode to 24 base64 characters");
            assert!(key.ends_with('='), "16 bytes leave one pad character");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_and_port_come_out_of_a_base_url() {
        assert_eq!(
            split_host_port("http://127.0.0.1:42617"),
            Some(("127.0.0.1".to_owned(), 42617))
        );
        assert_eq!(
            split_host_port("http://127.0.0.1:8080/prefix"),
            Some(("127.0.0.1".to_owned(), 8080))
        );
        assert_eq!(split_host_port("http://127.0.0.1"), None);
    }
}
