use crate::KernelError;

pub struct SseDecoder {
    pending: Vec<u8>,
    max_frame: usize,
}

impl SseDecoder {
    pub fn new(max_frame: usize) -> Self {
        Self {
            pending: Vec::with_capacity(max_frame.min(8_192)),
            max_frame,
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<String>, KernelError> {
        self.pending.extend_from_slice(chunk);
        let mut events = Vec::new();
        loop {
            let Some(end) = frame_end(&self.pending) else {
                if self.pending.len() > self.max_frame {
                    return Err(KernelError::Executor(format!(
                        "model SSE frame exceeded {} bytes",
                        self.max_frame
                    )));
                }
                break;
            };
            let frame: Vec<u8> = self.pending.drain(..end.length).collect();
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
