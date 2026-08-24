//! The B1 loop-host wire: one multiplexed duplex connection per loop-host process, carrying
//! every activation the brain runs there. Frames are u32 big-endian length-prefixed JSON with
//! a 2 MiB cap, following the environment-wire pattern (id-correlated request/response frames).
//!
//! Directions — brain→host: `hello`, `activate`, `abort`, `ctx_result`; host→brain:
//! `hello_ack`, `ctx`, `activation_result`. Activation ids and ctx ids are connection-scoped;
//! ctx ids correlate a `ctx` request with its `ctx_result` reply.

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The B1 frame cap. Ctx-op payloads are already bounded tighter (768 KiB) by the host.
pub const MAX_FRAME_BYTES: usize = 2 << 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Frame {
    /// First brain→host frame on a connection; a wrong token drops the connection.
    Hello { token: String },
    /// Host→brain: the token was accepted — the connection is live.
    HelloAck,
    /// Brain→host: run one guest activation.
    Activate {
        activation: u64,
        activation_kind: String,
        payload: String,
    },
    /// Brain→host: hard-stop a running activation (the brain-side turn is gone). The daemon
    /// drops the guest instance; no `activation_result` follows.
    Abort { activation: u64 },
    /// Host→brain: a guest ctx op for the turn that owns `activation`.
    Ctx {
        id: u64,
        activation: u64,
        payload: String,
    },
    /// Brain→host: the reply to `Ctx` with the same `id`.
    CtxResult { id: u64, payload: String },
    /// Host→brain: the activation finished — a guest verdict, or an error naming what the
    /// guest did (trapped, failed to instantiate).
    ActivationResult {
        activation: u64,
        verdict: Option<String>,
        error: Option<String>,
    },
}

impl Frame {
    /// The frame kind for logs and errors — never the payload, which may hold customer data.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Frame::Hello { .. } => "hello",
            Frame::HelloAck => "hello_ack",
            Frame::Activate { .. } => "activate",
            Frame::Abort { .. } => "abort",
            Frame::Ctx { .. } => "ctx",
            Frame::CtxResult { .. } => "ctx_result",
            Frame::ActivationResult { .. } => "activation_result",
        }
    }
}

pub async fn read_frame(reader: &mut (impl AsyncRead + Unpin)) -> std::io::Result<Frame> {
    let mut len_bytes = [0u8; 4];
    reader.read_exact(&mut len_bytes).await?;
    let len = u32::from_be_bytes(len_bytes) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("frame of {len} bytes exceeds the {MAX_FRAME_BYTES}-byte wire cap"),
        ));
    }
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload).await?;
    serde_json::from_slice(&payload).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("frame is not valid wire JSON: {error}"),
        )
    })
}

pub async fn write_frame(
    writer: &mut (impl AsyncWrite + Unpin),
    frame: &Frame,
) -> std::io::Result<()> {
    let payload = serde_json::to_vec(frame)?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "outbound {} frame of {} bytes exceeds the {MAX_FRAME_BYTES}-byte wire cap",
                frame.kind_name(),
                payload.len()
            ),
        ));
    }
    writer
        .write_all(&(payload.len() as u32).to_be_bytes())
        .await?;
    writer.write_all(&payload).await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn round_trip(frame: Frame) -> Frame {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, &frame).await.expect("write");
        read_frame(&mut buffer.as_slice()).await.expect("read")
    }

    #[tokio::test]
    async fn frames_round_trip() {
        for frame in [
            Frame::Hello { token: "t".into() },
            Frame::HelloAck,
            Frame::Activate {
                activation: 7,
                activation_kind: "message".into(),
                payload: "{}".into(),
            },
            Frame::Abort { activation: 7 },
            Frame::Ctx {
                id: 3,
                activation: 7,
                payload: r#"{"op":"engine.budget"}"#.into(),
            },
            Frame::CtxResult {
                id: 3,
                payload: r#"{"rounds":0}"#.into(),
            },
            Frame::ActivationResult {
                activation: 7,
                verdict: Some(r#"{"stop_reason":"end_turn"}"#.into()),
                error: None,
            },
        ] {
            let expected = serde_json::to_string(&frame).expect("json");
            let back = round_trip(frame).await;
            assert_eq!(serde_json::to_string(&back).expect("json"), expected);
        }
    }

    #[tokio::test]
    async fn oversized_frames_are_refused_in_both_directions() {
        let oversized = Frame::CtxResult {
            id: 1,
            payload: "x".repeat(MAX_FRAME_BYTES),
        };
        let mut buffer = Vec::new();
        let written = write_frame(&mut buffer, &oversized).await;
        assert_eq!(
            written.expect_err("cap").kind(),
            std::io::ErrorKind::InvalidData
        );

        let mut wire = ((MAX_FRAME_BYTES + 1) as u32).to_be_bytes().to_vec();
        wire.extend_from_slice(b"{");
        let read = read_frame(&mut wire.as_slice()).await;
        assert_eq!(
            read.expect_err("cap").kind(),
            std::io::ErrorKind::InvalidData
        );
    }

    #[tokio::test]
    async fn a_torn_frame_reads_as_eof() {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, &Frame::HelloAck)
            .await
            .expect("write");
        buffer.truncate(buffer.len() - 1);
        let read = read_frame(&mut buffer.as_slice()).await;
        assert_eq!(
            read.expect_err("torn").kind(),
            std::io::ErrorKind::UnexpectedEof
        );
    }
}
