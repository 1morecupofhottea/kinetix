//! A streaming SSE frame splitter that tolerates arbitrary transport chunk
//! boundaries (FR-2.12).
//!
//! Providers separate SSE frames with either `\n\n` or `\r\n\r\n`; a single
//! upstream TCP read may deliver half a frame, several frames, or split a
//! multi-byte UTF-8 sequence. `SseFramer` accumulates *raw bytes* (never doing
//! a lossy per-chunk conversion, which would corrupt a split code point),
//! normalizes CRLF to LF, and yields complete frames — never a partial one.
//! This is exercised by the protocol torture tests in `src/torture.rs`.

/// Splits an inbound SSE byte stream into complete frames.
#[derive(Default)]
pub struct SseFramer {
    buffer: Vec<u8>,
}

impl SseFramer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one transport chunk (any byte boundary) and return every complete
    /// frame it completed, in order. Frames keep their trailing blank line
    /// stripped but are otherwise verbatim (comments, `event:`, `data:` lines).
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        // Normalize CRLF -> LF on raw bytes so a split CRLF pair is handled
        // correctly regardless of where the chunk boundary falls.
        for &b in bytes {
            self.buffer.push(b);
        }
        self.buffer = normalize_crlf(&self.buffer);

        let mut frames = Vec::new();
        while let Some(idx) = find_double_lf(&self.buffer) {
            let frame_bytes: Vec<u8> = self.buffer.drain(..idx + 2).collect();
            // Strip the trailing blank line.
            let mut end = frame_bytes.len();
            while end > 0 && (frame_bytes[end - 1] == b'\n' || frame_bytes[end - 1] == b'\r') {
                end -= 1;
            }
            if end == 0 {
                continue;
            }
            // A frame boundary only occurs between complete code points, so this
            // is guaranteed valid UTF-8.
            frames.push(String::from_utf8_lossy(&frame_bytes[..end]).to_string());
        }
        frames
    }

    /// Whether a partial frame is buffered awaiting more bytes.
    pub fn pending(&self) -> bool {
        !self.buffer.is_empty()
    }

    /// Take whatever is buffered (used at end-of-stream).
    pub fn flush(&mut self) -> Option<String> {
        let mut end = self.buffer.len();
        while end > 0 && (self.buffer[end - 1] == b'\n' || self.buffer[end - 1] == b'\r') {
            end -= 1;
        }
        let rest = String::from_utf8_lossy(&self.buffer[..end]).to_string();
        self.buffer.clear();
        if rest.is_empty() {
            None
        } else {
            Some(rest)
        }
    }
}

/// Replace every `\r\n` with `\n`, and a lone trailing `\r` is left in place
/// (it may be the first half of a CRLF whose `\n` arrives in the next chunk —
/// the next call's normalization handles it because the `\r` stays buffered).
fn normalize_crlf(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                out.push(b'\n');
                i += 2;
                continue;
            }
            if i + 1 == bytes.len() {
                // Trailing CR: defer — keep it so a following LF can pair with
                // it on the next push.
                out.push(b'\r');
                i += 1;
                continue;
            }
            // A CR not followed by LF: emit as-is.
            out.push(b'\r');
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

/// Find the index of the first `\n\n` in the buffer.
fn find_double_lf(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 2 {
        return None;
    }
    for i in 0..bytes.len() - 1 {
        if bytes[i] == b'\n' && bytes[i + 1] == b'\n' {
            return Some(i);
        }
    }
    None
}

/// Extract the `data:` payload from one SSE frame, joining multiple data lines
/// with newlines per the SSE spec.
pub fn extract_data(frame: &str) -> Option<String> {
    let mut data = String::new();
    let mut found = false;
    for line in frame.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            found = true;
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }
    if found {
        Some(data)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_in_chunks(framer: &mut SseFramer, input: &str, size: usize) -> Vec<String> {
        let bytes = input.as_bytes();
        let mut out = Vec::new();
        for chunk in bytes.chunks(size) {
            out.extend(framer.push(chunk));
        }
        out
    }

    #[test]
    fn splits_lf_frames() {
        let mut f = SseFramer::new();
        let frames = f.push(b"data: a\n\ndata: b\n\n");
        assert_eq!(frames, vec!["data: a", "data: b"]);
        assert!(!f.pending());
    }

    #[test]
    fn splits_crlf_frames() {
        let mut f = SseFramer::new();
        let frames = f.push(b"data: a\r\n\r\ndata: b\r\n\r\n");
        assert_eq!(frames, vec!["data: a", "data: b"]);
    }

    #[test]
    fn tolerates_one_byte_chunks() {
        let input = "data: {\"x\":1}\n\ndata: {\"y\":2}\n\n: keepalive\n\ndata: [DONE]\n\n";
        for size in 1..=8 {
            let mut f = SseFramer::new();
            let frames = feed_in_chunks(&mut f, input, size);
            assert_eq!(
                frames,
                vec![
                    "data: {\"x\":1}",
                    "data: {\"y\":2}",
                    ": keepalive",
                    "data: [DONE]"
                ],
                "chunk size {size}"
            );
        }
    }

    #[test]
    fn tolerates_split_utf8_multibyte() {
        // A multi-byte UTF-8 sequence split across two chunks must not corrupt.
        let input = "data: {\"t\":\"héllo→世界\"}\n\n";
        let bytes = input.as_bytes();
        for split in 1..bytes.len() {
            let mut f = SseFramer::new();
            let mut frames = f.push(&bytes[..split]);
            frames.extend(f.push(&bytes[split..]));
            assert_eq!(
                frames,
                vec!["data: {\"t\":\"héllo→世界\"}"],
                "split {split}"
            );
        }
    }

    #[test]
    fn crlf_split_across_chunks() {
        // The CRLF terminator split so that CR ends one chunk and LF starts the
        // next must still be recognized.
        let mut f = SseFramer::new();
        assert!(f.push(b"data: a\r\n\r").is_empty());
        assert_eq!(f.push(b"\ndata: b\r\n\r\n"), vec!["data: a", "data: b"]);
    }

    #[test]
    fn partial_frame_is_not_emitted() {
        let mut f = SseFramer::new();
        assert!(f.push(b"data: par").is_empty());
        assert!(f.pending());
        let frames = f.push(b"tial\n\n");
        assert_eq!(frames, vec!["data: partial"]);
    }

    #[test]
    fn joins_multiple_data_lines() {
        let mut f = SseFramer::new();
        let frames = f.push(b"data: line1\ndata: line2\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(extract_data(&frames[0]).unwrap(), "line1\nline2");
    }

    #[test]
    fn comments_have_no_data() {
        let mut f = SseFramer::new();
        let frames = f.push(b": keepalive\n\n");
        assert_eq!(frames, vec![": keepalive"]);
        assert!(extract_data(&frames[0]).is_none());
    }
}
