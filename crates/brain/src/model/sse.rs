use crate::KernelError;

pub struct SseDecoder {
    pending: Vec<u8>,
    /// How far into `pending` the separator scan has already looked. A model delivers
    /// one frame across many network chunks, so rescanning from the start of the buffer
    /// on every chunk costs the square of the chunk count.
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
            scanned: 0,
            max_frame,
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<String>, KernelError> {
        self.pending.extend_from_slice(chunk);
        let mut events = Vec::new();
        loop {
            let from = self.scanned.saturating_sub(SEPARATOR_OVERLAP);
            let found = frame_end(&self.pending[from..]).map(|end| FrameEnd {
                content: end.content + from,
                length: end.length + from,
            });
            let Some(end) = found else {
                self.scanned = self.pending.len();
                if self.pending.len() > self.max_frame {
                    return Err(KernelError::Executor(format!(
                        "model SSE frame exceeded {} bytes",
                        self.max_frame
                    )));
                }
                break;
            };
            let frame: Vec<u8> = self.pending.drain(..end.length).collect();
            // What remains starts a fresh frame, so the next scan starts at its start.
            self.scanned = 0;
            let text = std::str::from_utf8(&frame[..end.content]).map_err(|error| {
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
        }
        Ok(events)
    }

    pub fn pending_bytes(&self) -> usize {
        self.pending.len()
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

    #[test]
    fn bounds_unterminated_frames() {
        let mut decoder = SseDecoder::new(8);
        assert!(decoder.feed(b"12345678").is_ok());
        assert!(decoder.feed(b"9").is_err());
    }
}
