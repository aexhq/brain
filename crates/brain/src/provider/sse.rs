//! An incremental SSE decoder.
//!
//! Bounded at construction: a frame that exceeds `max_frame` is a typed error,
//! not an unbounded buffer. Brain forbids unbounded buffers, and a provider
//! that never sends a blank line is otherwise an OOM.
//!
//! Handles the required wire cases: fragmentation
//! at any byte boundary, split UTF-8, comment lines, CRLF, and early EOF.

use crate::{BrainError, Result};

pub const DEFAULT_MAX_FRAME: usize = 256 * 1024;
const MAX_EVENTS_PER_FEED: usize = 4096;

#[derive(Debug)]
pub struct SseDecoder {
    buf: Vec<u8>,
    line: Vec<u8>,
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
            line: Vec::with_capacity(1024),
            max_frame: DEFAULT_MAX_FRAME,
        }
    }
}

impl SseDecoder {
    pub fn with_max_frame(max_frame: usize) -> Self {
        SseDecoder {
            buf: Vec::with_capacity(max_frame.min(8192)),
            line: Vec::with_capacity(max_frame.min(1024)),
            max_frame,
        }
    }

    /// Feed bytes; drain whatever complete events they completed.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>> {
        let mut out = Vec::new();
        for &byte in chunk {
            if byte != b'\n' {
                if self.pending() >= self.max_frame {
                    return Err(self.frame_too_large());
                }
                self.line.push(byte);
                continue;
            }

            let normalized = self
                .line
                .strip_suffix(b"\r")
                .unwrap_or(self.line.as_slice());
            if normalized.is_empty() {
                if let Some(event) = parse_frame(&self.buf)? {
                    if out.len() >= MAX_EVENTS_PER_FEED {
                        return Err(BrainError::Protocol(format!(
                            "provider chunk contained more than {MAX_EVENTS_PER_FEED} SSE events"
                        )));
                    }
                    out.push(event);
                }
                self.buf.clear();
            } else {
                let separator = usize::from(!self.buf.is_empty());
                if self
                    .buf
                    .len()
                    .saturating_add(separator)
                    .saturating_add(normalized.len())
                    > self.max_frame
                {
                    return Err(self.frame_too_large());
                }
                if separator == 1 {
                    self.buf.push(b'\n');
                }
                self.buf.extend_from_slice(normalized);
            }
            self.line.clear();
        }
        Ok(out)
    }

    fn frame_too_large(&self) -> BrainError {
        BrainError::Protocol(format!(
            "SSE frame exceeded {} bytes without a terminator",
            self.max_frame
        ))
    }

    /// Bytes still buffered at EOF. Non-empty means the stream ended mid-frame,
    /// which the caller reports rather than silently discarding.
    pub fn pending(&self) -> usize {
        self.buf.len().saturating_add(self.line.len())
    }
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
        assert!(d.feed(&[b'x'; 64]).unwrap().is_empty());
        assert_eq!(d.pending(), 64);
        let err = d.feed(b"x").unwrap_err();
        assert!(matches!(err, BrainError::Protocol(_)));
        assert_eq!(
            d.pending(),
            64,
            "the rejected byte must never enter the buffer"
        );
        assert!(d.buf.capacity() <= 64);
        assert!(d.line.capacity() <= 64);
    }

    #[test]
    fn early_eof_is_visible_to_the_caller() {
        let mut d = SseDecoder::default();
        assert!(d.feed(b"data: half").unwrap().is_empty());
        assert_eq!(d.pending(), 10, "a truncated stream must not look complete");
    }

    #[test]
    fn one_large_http_chunk_may_contain_many_small_frames() {
        let frame = format!("data: {{\"value\":\"{}\"}}\n\n", "x".repeat(128));
        let chunk = frame.as_bytes().repeat(2_048);
        assert!(chunk.len() > DEFAULT_MAX_FRAME);
        let mut decoder = SseDecoder::default();
        let events = decoder.feed(&chunk).unwrap();
        assert_eq!(events.len(), 2_048);
        assert_eq!(decoder.pending(), 0);
    }

    #[test]
    fn a_crlf_terminator_split_across_every_byte_stays_valid() {
        let wire = b"event: answer\r\ndata: yes\r\n\r\n";
        for split in 1..wire.len() {
            let mut decoder = SseDecoder::default();
            let mut events = decoder.feed(&wire[..split]).unwrap();
            events.extend(decoder.feed(&wire[split..]).unwrap());
            assert_eq!(events.len(), 1, "split {split}");
            assert_eq!(events[0].data, "yes");
            assert_eq!(decoder.pending(), 0);
        }
    }
}
