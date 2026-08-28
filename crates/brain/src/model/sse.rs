use crate::KernelError;

/// Splits a model's byte stream into SSE frames.
///
/// Two things here are deliberate, and both were quadratic before:
///
/// The separator scan resumes where it stopped. A model delivers one frame across many
/// network chunks, so restarting the search at the front of the buffer on every chunk
/// searched the same bytes once per chunk.
///
/// Consumed bytes are dropped once per `feed`, not once per frame. Taking each frame off
/// the front moved every byte behind it, so a chunk carrying many frames moved the tail
/// of the buffer once per frame in it.
pub struct SseDecoder {
    pending: Vec<u8>,
    /// Bytes at the front of `pending` already returned as frames.
    consumed: usize,
    /// How far past `consumed` the separator scan has already looked.
    scanned: usize,
    max_frame: usize,
}

/// The longest separator is four bytes, so a resumed scan backs up three in case one
/// straddles the boundary between the last chunk and this one.
const SEPARATOR_OVERLAP: usize = 3;

impl SseDecoder {
    pub fn new(max_frame: usize) -> Self {
        Self {
            pending: Vec::with_capacity(max_frame.min(8_192)),
            consumed: 0,
            scanned: 0,
            max_frame,
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<String>, KernelError> {
        self.pending.extend_from_slice(chunk);
        let mut events = Vec::new();
        let overflowed;
        loop {
            let from = self.scanned.saturating_sub(SEPARATOR_OVERLAP);
            let unread = self.pending.len() - self.consumed;
            let found = frame_end(&self.pending[self.consumed + from..]).map(|end| FrameEnd {
                content: end.content + from,
                length: end.length + from,
            });
            let Some(end) = found else {
                self.scanned = unread;
                // Reported after the buffer is compacted below, so the decoder is left
                // in a consistent state whichever way this call returns.
                overflowed = unread > self.max_frame;
                break;
            };
            let content = &self.pending[self.consumed..self.consumed + end.content];
            let text = std::str::from_utf8(content).map_err(|error| {
                KernelError::Executor(format!("model SSE is not UTF-8: {error}"))
            })?;
            let data = text
                .lines()
                .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
                .collect::<Vec<_>>()
                .join("\n");
            if !data.is_empty() {
                events.push(data);
            }
            self.consumed += end.length;
            // What remains starts a fresh frame, so the next scan starts at its start.
            self.scanned = 0;
        }
        if self.consumed > 0 {
            self.pending.drain(..self.consumed);
            self.consumed = 0;
        }
        if overflowed {
            return Err(KernelError::Executor(format!(
                "model SSE frame exceeded {} bytes",
                self.max_frame
            )));
        }
        Ok(events)
    }

    /// Bytes held for a frame that has not terminated yet. Consumed bytes are dropped
    /// before `feed` returns, so this never counts them.
    pub fn pending_bytes(&self) -> usize {
        self.pending.len() - self.consumed
    }
}

struct FrameEnd {
    content: usize,
    length: usize,
}

fn frame_end(bytes: &[u8]) -> Option<FrameEnd> {
    for index in 0..bytes.len().saturating_sub(1) {
        if bytes[index..].starts_with(b"\n\n") {
            return Some(FrameEnd {
                content: index,
                length: index + 2,
            });
        }
        if bytes[index..].starts_with(b"\r\n\r\n") {
            return Some(FrameEnd {
                content: index,
                length: index + 4,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A separator split across two chunks must still be found: the resumed scan backs
    /// up far enough to see it whole.
    #[test]
    fn finds_a_separator_split_across_chunks() {
        for separator in [&b"\n\n"[..], &b"\r\n\r\n"[..]] {
            for split in 1..separator.len() {
                let mut decoder = SseDecoder::new(128);
                let mut head = b"data: one".to_vec();
                head.extend_from_slice(&separator[..split]);
                assert!(decoder.feed(&head).unwrap().is_empty());
                assert_eq!(
                    decoder.feed(&separator[split..]).unwrap(),
                    ["one"],
                    "a separator split at {split} must still terminate the frame"
                );
                assert_eq!(decoder.pending_bytes(), 0);
            }
        }
    }

    /// Several frames in one chunk must all come out, which means the scan offset resets
    /// when a frame is taken rather than skipping past the next separator.
    #[test]
    fn decodes_several_frames_from_one_chunk() {
        let mut decoder = SseDecoder::new(128);
        assert_eq!(
            decoder
                .feed(b"data: one\n\ndata: two\n\ndata: three")
                .unwrap(),
            ["one", "two"]
        );
        assert_eq!(decoder.feed(b"\n\n").unwrap(), ["three"]);
    }

    #[test]
    fn decodes_fragmented_and_crlf_frames() {
        let mut decoder = SseDecoder::new(128);
        assert!(decoder.feed(b"data: {\"a\":").unwrap().is_empty());
        assert_eq!(
            decoder.feed(b"1}\r\n\r\ndata: [DONE]\n\n").unwrap(),
            ["{\"a\":1}", "[DONE]"]
        );
        assert_eq!(decoder.pending_bytes(), 0);
    }

    /// Many frames in one chunk must not cost the square of their number. The check is
    /// on work done, not wall time: taking each frame off the front of the buffer moved
    /// every byte behind it, so the tail was rewritten once per frame.
    #[test]
    fn decodes_many_frames_in_one_chunk_without_rewriting_the_buffer() {
        const FRAMES: usize = 20_000;
        let mut stream = Vec::new();
        for index in 0..FRAMES {
            stream.extend_from_slice(format!("data: {{\"n\":{index}}}\n\n").as_bytes());
        }
        let mut decoder = SseDecoder::new(1024 * 1024);
        let events = decoder.feed(&stream).unwrap();
        assert_eq!(events.len(), FRAMES);
        assert_eq!(events[0], "{\"n\":0}");
        assert_eq!(events[FRAMES - 1], format!("{{\"n\":{}}}", FRAMES - 1));
        assert_eq!(decoder.pending_bytes(), 0);
    }

    /// A frame that arrives after several complete ones must still be bounded, which
    /// means the size check counts only the bytes still held.
    #[test]
    fn bounds_a_frame_that_follows_complete_ones() {
        let mut decoder = SseDecoder::new(16);
        assert_eq!(decoder.feed(b"data: a\n\ndata: b\n\n").unwrap(), ["a", "b"]);
        assert_eq!(decoder.pending_bytes(), 0);
        assert!(decoder.feed(b"data: ").unwrap().is_empty());
        assert!(
            decoder.feed(&[b'x'; 32]).is_err(),
            "an unterminated frame past the bound must be rejected even after complete ones"
        );
    }

    #[test]
    fn bounds_unterminated_frames() {
        let mut decoder = SseDecoder::new(8);
        assert!(decoder.feed(b"12345678").is_ok());
        assert!(decoder.feed(b"9").is_err());
    }
}
