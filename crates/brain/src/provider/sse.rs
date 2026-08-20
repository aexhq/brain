//! An incremental SSE decoder.
//!
//! Bounded at construction: a frame that exceeds `max_frame` is a typed error,
//! not an unbounded buffer. Brain forbids unbounded buffers, and a provider
//! that never sends a blank line is otherwise an OOM.
//!
//! Handles the required wire cases: fragmentation
//! at any byte boundary, split UTF-8, comment lines, CRLF, and early EOF.

use crate::{BrainError, Result};

pub const DEFAULT_MAX_FRAME: usize = 1024 * 1024;

#[derive(Debug)]
pub struct SseDecoder {
    buf: Vec<u8>,
    max_frame: usize,
}

#[derive(Debug, PartialEq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

impl Default for SseDecoder {
    fn default() -> Self {
        SseDecoder {
            buf: Vec::with_capacity(8192),
            max_frame: DEFAULT_MAX_FRAME,
        }
    }
}

impl SseDecoder {
    pub fn with_max_frame(max_frame: usize) -> Self {
        SseDecoder {
            buf: Vec::with_capacity(8192),
            max_frame,
        }
    }

    /// Feed bytes; drain whatever complete events they completed.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>> {
        self.buf.extend_from_slice(chunk);
        if self.buf.len() > self.max_frame {
            return Err(BrainError::Protocol(format!(
                "SSE frame exceeded {} bytes without a terminator",
                self.max_frame
            )));
        }
        let mut out = Vec::new();
        while let Some(end) = find_terminator(&self.buf) {
            let (frame, rest) = self.buf.split_at(end.0);
            let ev = parse_frame(frame)?;
            let rest = rest[end.1..].to_vec();
            self.buf = rest;
            if let Some(ev) = ev {
                out.push(ev);
            }
        }
        Ok(out)
    }

    /// Bytes still buffered at EOF. Non-empty means the stream ended mid-frame,
    /// which the caller reports rather than silently discarding.
    pub fn pending(&self) -> usize {
        self.buf.len()
    }
}

/// Returns (offset of terminator, terminator length).
fn find_terminator(buf: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i + 1 < buf.len() {
        if buf[i] == b'\n' && buf[i + 1] == b'\n' {
            return Some((i, 2));
        }
        if i + 3 < buf.len()
            && buf[i] == b'\r'
            && buf[i + 1] == b'\n'
            && buf[i + 2] == b'\r'
            && buf[i + 3] == b'\n'
        {
            return Some((i, 4));
        }
        i += 1;
    }
    None
}

fn parse_frame(frame: &[u8]) -> Result<Option<SseEvent>> {
    let mut event = None;
    let mut data = String::new();
    for line in frame.split(|b| *b == b'\n') {
        let line = if line.last() == Some(&b'\r') {
            &line[..line.len() - 1]
        } else {
            line
        };
        if line.is_empty() || line.first() == Some(&b':') {
            // Blank or comment (including the `: keep-alive` heartbeats several
            // gateways send).
            continue;
        }
        let (field, value) = match line.iter().position(|b| *b == b':') {
            Some(p) => {
                let v = &line[p + 1..];
                let v = if v.first() == Some(&b' ') { &v[1..] } else { v };
                (&line[..p], v)
            }
            None => (line, &b""[..]),
        };
        // Split UTF-8 across chunk boundaries is impossible here: we only ever
        // parse a frame once its terminator has arrived, so the frame is
        // complete. Invalid UTF-8 inside a complete frame is a real protocol
        // error and is reported as one.
        let value = std::str::from_utf8(value)
            .map_err(|e| BrainError::Protocol(format!("SSE value is not UTF-8: {e}")))?;
        match field {
            b"event" => event = Some(value.to_string()),
            b"data" => {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(value);
            }
            _ => {}
        }
    }
    if event.is_none() && data.is_empty() {
        return Ok(None);
    }
    Ok(Some(SseEvent { event, data }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_simple_stream() {
        let mut d = SseDecoder::default();
        let evs = d
            .feed(b"event: ping\ndata: {\"a\":1}\n\nevent: pong\ndata: {}\n\n")
            .unwrap();
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].event.as_deref(), Some("ping"));
        assert_eq!(evs[0].data, "{\"a\":1}");
        assert_eq!(evs[1].event.as_deref(), Some("pong"));
    }

    #[test]
    fn survives_fragmentation_at_every_byte_boundary() {
        let full = b"event: message_delta\ndata: {\"x\": \"\xc3\xa9\"}\n\ndata: [DONE]\n\n";
        for split in 1..full.len() {
            let mut d = SseDecoder::default();
            let mut all = d.feed(&full[..split]).unwrap();
            all.extend(d.feed(&full[split..]).unwrap());
            assert_eq!(all.len(), 2, "split at {split} lost an event");
            assert_eq!(all[1].data, "[DONE]");
            assert_eq!(d.pending(), 0, "split at {split} left residue");
        }
    }

    #[test]
    fn skips_comments_and_keepalives() {
        let mut d = SseDecoder::default();
        let evs = d.feed(b": keep-alive\n\ndata: real\n\n").unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].data, "real");
    }

    #[test]
    fn multiline_data_is_newline_joined() {
        let mut d = SseDecoder::default();
        let evs = d.feed(b"data: line1\ndata: line2\n\n").unwrap();
        assert_eq!(evs[0].data, "line1\nline2");
    }

    #[test]
    fn crlf_terminated_frames_work() {
        let mut d = SseDecoder::default();
        let evs = d.feed(b"event: a\r\ndata: b\r\n\r\n").unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].data, "b");
    }

    #[test]
    fn an_unterminated_frame_is_bounded_not_unbounded() {
        let mut d = SseDecoder::with_max_frame(64);
        let err = d.feed(&[b'x'; 65]).unwrap_err();
        assert!(matches!(err, BrainError::Protocol(_)));
    }

    #[test]
    fn early_eof_is_visible_to_the_caller() {
        let mut d = SseDecoder::default();
        assert!(d.feed(b"data: half").unwrap().is_empty());
        assert_eq!(d.pending(), 10, "a truncated stream must not look complete");
    }
}
