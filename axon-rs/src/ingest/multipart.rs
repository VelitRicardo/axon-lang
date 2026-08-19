//! Minimal `multipart/form-data` parser that streams each part
//! into a [`crate::buffer::BufferMut`].
//!
//! The parser works on discrete chunks (as they arrive from the
//! network) so ingest never buffers the entire request body in
//! RAM; each part transitions from `BufferMut` to `ZeroCopyBuffer`
//! when its boundary is hit.
//!
//! This is a pragmatic RFC 7578 subset. It handles:
//!
//! - Boundary detection (leading + closing)
//! - Header parsing (case-insensitive on names)
//! - `Content-Disposition` → field name + optional file name
//! - `Content-Type` → informational, plus a best-effort mapping to
//!   the [`crate::buffer::BufferKind`] tag
//! - Streaming payload accumulation into `BufferMut`
//!
//! Out of scope for 11.b:
//!
//! - `Content-Transfer-Encoding` (base64, quoted-printable) — adopters
//!   that need these decode at the application layer
//! - Nested multipart (multipart within multipart) — rejected with
//!   `MultipartError::Nested`
//!
//! The API is stepwise (`feed(bytes) -> Vec<Event>`) so the caller
//! — typically a Tokio HTTP handler — drives the parser without
//! owning the full request.

use crate::buffer::{BufferKind, BufferMut, ZeroCopyBuffer};

// ── Errors ───────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum MultipartError {
    /// Content-Type boundary missing or malformed.
    MissingBoundary,
    /// Header section exceeded our per-part cap.
    HeaderTooLarge {
        limit: usize,
    },
    /// Single part payload exceeded the configured per-part limit.
    PartTooLarge {
        limit: usize,
    },
    /// Upstream closed the connection mid-part.
    UnexpectedEof,
    /// Nested multipart bodies are not supported.
    Nested,
    /// Malformed header line (no `:` separator).
    MalformedHeader {
        line: String,
    },
}

impl std::fmt::Display for MultipartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingBoundary => write!(f, "missing multipart boundary"),
            Self::HeaderTooLarge { limit } => {
                write!(f, "header section exceeded {limit} bytes")
            }
            Self::PartTooLarge { limit } => {
                write!(f, "part payload exceeded {limit} bytes")
            }
            Self::UnexpectedEof => write!(f, "stream ended mid-part"),
            Self::Nested => write!(f, "nested multipart is not supported"),
            Self::MalformedHeader { line } => {
                write!(f, "malformed header: {line:?}")
            }
        }
    }
}

impl std::error::Error for MultipartError {}

// ── Events emitted by the parser ─────────────────────────────────────

/// What the caller learns after feeding a chunk. The parser may emit
/// multiple events per chunk (a boundary can appear mid-chunk).
#[derive(Debug)]
pub enum MultipartEvent {
    /// Start of a new part. Carries metadata extracted from headers.
    PartStart {
        field_name: String,
        file_name: Option<String>,
        content_type: Option<String>,
        kind: BufferKind,
    },
    /// End of the current part — the accumulated payload is now
    /// available as an immutable [`ZeroCopyBuffer`].
    PartEnd {
        field_name: String,
        payload: ZeroCopyBuffer,
    },
    /// Terminating boundary — no more parts will follow.
    Complete,
}

// ── Configuration ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct MultipartLimits {
    pub max_header_bytes: usize,
    pub max_part_bytes: usize,
}

impl Default for MultipartLimits {
    fn default() -> Self {
        // 16 KiB headers should fit any reasonable Content-Disposition.
        // 32 MiB per part is a sensible web default; adopters with
        // larger uploads raise this explicitly.
        MultipartLimits {
            max_header_bytes: 16 * 1024,
            max_part_bytes: 32 * 1024 * 1024,
        }
    }
}

// ── Parser ───────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
enum State {
    /// Before the first boundary — we tolerate preamble.
    Preamble,
    /// Collecting header lines of the current part.
    Headers,
    /// Streaming payload bytes into the current part.
    Body,
    /// We saw the terminating `--boundary--` marker.
    Terminated,
}

pub struct MultipartParser {
    boundary: Vec<u8>,
    limits: MultipartLimits,
    state: State,
    /// Accumulates bytes until we've matched a boundary or header end.
    buf: Vec<u8>,
    /// Headers for the current part (cleared on PartStart emission).
    current_headers: Vec<(String, String)>,
    /// Payload accumulator for the current part.
    current_body: Option<BufferMut>,
    current_field_name: Option<String>,
}

impl MultipartParser {
    /// Build a parser from the boundary extracted from the request's
    /// `Content-Type` header (callers pass the value WITHOUT the
    /// leading `--`; `parse_boundary_from_content_type` helps).
    pub fn new(boundary: impl Into<String>, limits: MultipartLimits) -> Self {
        let boundary = boundary.into().into_bytes();
        MultipartParser {
            boundary,
            limits,
            state: State::Preamble,
            buf: Vec::with_capacity(4 * 1024),
            current_headers: Vec::new(),
            current_body: None,
            current_field_name: None,
        }
    }

    /// Feed a chunk of bytes. May emit 0, 1 or many events; caller
    /// consumes them in order.
    pub fn feed(
        &mut self,
        chunk: &[u8],
    ) -> Result<Vec<MultipartEvent>, MultipartError> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        loop {
            let progressed = self.step(&mut out)?;
            if !progressed {
                break;
            }
        }
        Ok(out)
    }

    /// Called by the driver when the upstream closes. Emits any
    /// trailing part or an `UnexpectedEof` error if we're mid-part.
    pub fn finalize(
        &mut self,
        out: &mut Vec<MultipartEvent>,
    ) -> Result<(), MultipartError> {
        match self.state {
            State::Terminated => Ok(()),
            State::Preamble => Err(MultipartError::UnexpectedEof),
            State::Headers => Err(MultipartError::UnexpectedEof),
            State::Body => {
                // We treat an EOF-without-closing-boundary as the
                // remaining body — most clients produce a closing
                // boundary, but a resilient parser accepts the tail.
                if let Some(body) = self.current_body.take() {
                    let field = self.current_field_name.take().unwrap_or_default();
                    out.push(MultipartEvent::PartEnd {
                        field_name: field,
                        payload: body.freeze(),
                    });
                }
                self.state = State::Terminated;
                Ok(())
            }
        }
    }

    // ── Inner step ────────────────────────────────────────────────

    fn step(
        &mut self,
        out: &mut Vec<MultipartEvent>,
    ) -> Result<bool, MultipartError> {
        match self.state {
            State::Preamble => self.seek_initial_boundary(out),
            State::Headers => self.parse_headers(out),
            State::Body => self.stream_body(out),
            State::Terminated => Ok(false),
        }
    }

    fn seek_initial_boundary(
        &mut self,
        _out: &mut Vec<MultipartEvent>,
    ) -> Result<bool, MultipartError> {
        // The first boundary is `--<boundary>\r\n`.
        let marker = self.boundary_marker(/*closing=*/ false);
        match find_subsequence(&self.buf, &marker) {
            Some(idx) => {
                let tail = idx + marker.len();
                self.buf.drain(..tail);
                self.state = State::Headers;
                Ok(true)
            }
            None => {
                // Keep the trailing 4*max(2,marker.len()) bytes so a
                // split boundary across two feeds still matches on
                // the next step.
                let keep = marker.len().saturating_add(4);
                if self.buf.len() > keep {
                    self.buf.drain(..self.buf.len() - keep);
                }
                Ok(false)
            }
        }
    }

    fn parse_headers(
        &mut self,
        out: &mut Vec<MultipartEvent>,
    ) -> Result<bool, MultipartError> {
        // Header section terminates at the first blank line (`\r\n\r\n`).
        let terminator = b"\r\n\r\n";
        let Some(idx) = find_subsequence(&self.buf, terminator) else {
            if self.buf.len() > self.limits.max_header_bytes {
                return Err(MultipartError::HeaderTooLarge {
                    limit: self.limits.max_header_bytes,
                });
            }
            return Ok(false);
        };

        // v1.4.2 — also enforce `max_header_bytes` when the
        // terminator arrives in the same feed as the (oversized)
        // header. Without this, a caller that fills the buffer with
        // one big call bypasses the limit because the "buffer larger
        // than the cap" branch above only fires while the terminator
        // is still missing.
        if idx > self.limits.max_header_bytes {
            return Err(MultipartError::HeaderTooLarge {
                limit: self.limits.max_header_bytes,
            });
        }

        let header_block = self.buf.drain(..idx + terminator.len()).collect::<Vec<u8>>();
        // Drop the trailing blank-line terminator from the parse set.
        let header_text = &header_block[..header_block.len() - terminator.len()];
        let text = std::str::from_utf8(header_text).unwrap_or("");
        self.current_headers.clear();
        for raw in text.split("\r\n") {
            if raw.is_empty() {
                continue;
            }
            let Some((k, v)) = raw.split_once(':') else {
                return Err(MultipartError::MalformedHeader {
                    line: raw.to_string(),
                });
            };
            self.current_headers
                .push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }

        let (field_name, file_name) = disposition_field_and_file(
            &self.current_headers,
        );
        let content_type = self
            .current_headers
            .iter()
            .find(|(k, _)| k == "content-type")
            .map(|(_, v)| v.clone());

        if content_type
            .as_deref()
            .map(|ct| ct.to_ascii_lowercase().contains("multipart/"))
            .unwrap_or(false)
        {
            return Err(MultipartError::Nested);
        }

        let kind = kind_for_content_type(content_type.as_deref());

        out.push(MultipartEvent::PartStart {
            field_name: field_name.clone().unwrap_or_default(),
            file_name,
            content_type,
            kind: kind.clone(),
        });

        self.current_field_name = field_name;
        self.current_body = Some(BufferMut::with_capacity(4 * 1024, kind));
        self.state = State::Body;
        Ok(true)
    }

    fn stream_body(
        &mut self,
        out: &mut Vec<MultipartEvent>,
    ) -> Result<bool, MultipartError> {
        let open_marker = self.boundary_marker(false);
        let close_marker = self.boundary_marker(true);

        // Find the earliest marker in the buffer.
        let open_idx = find_subsequence(&self.buf, &open_marker);
        let close_idx = find_subsequence(&self.buf, &close_marker);

        let (boundary_idx, is_closing) = match (open_idx, close_idx) {
            (None, None) => (None, false),
            (Some(o), None) => (Some(o), false),
            (None, Some(c)) => (Some(c), true),
            (Some(o), Some(c)) => {
                if c < o {
                    (Some(c), true)
                } else {
                    (Some(o), false)
                }
            }
        };

        let Some(idx) = boundary_idx else {
            // No boundary in buffer yet. Flush everything EXCEPT the
            // trailing tail that might still become either:
            //   · a boundary marker (up to `close_marker.len()` bytes), or
            // · the mandatory `\r\n` that RFC 7578 section 4.1 requires
            //     immediately before the boundary (body_end = idx - 2
            //     trims those two bytes from the body, but only if
            //     they are still in the buffer when the marker is
            //     recognised).
            //
            // v1.4.2 fix — the previous heuristic kept only
            // `close_marker.len()` (or `open_marker.len()` when the
            // buffer was smaller). In a byte-at-a-time feed that lost
            // the `\r\n` preceding the boundary, emitting it as part
            // of the body and leaving the boundary marker
            // unrecognisable because the parser had already flushed
            // the first one or two bytes of its prefix.
            let keep = close_marker.len().max(open_marker.len()) + 2;
            if self.buf.len() <= keep {
                return Ok(false);
            }
            let take = self.buf.len() - keep;
            let body = self
                .current_body
                .as_mut()
                .expect("body builder missing in Body state");
            if body.len() + take > self.limits.max_part_bytes {
                return Err(MultipartError::PartTooLarge {
                    limit: self.limits.max_part_bytes,
                });
            }
            body.extend_from_slice(&self.buf[..take]);
            self.buf.drain(..take);
            return Ok(false);
        };

        // The body ends at `idx - 2` to trim the trailing `\r\n` that
        // precedes every boundary (RFC 7578 section 4.1).
        let body_end = idx.saturating_sub(2);
        {
            let body = self
                .current_body
                .as_mut()
                .expect("body builder missing in Body state");
            if body.len() + body_end > self.limits.max_part_bytes {
                return Err(MultipartError::PartTooLarge {
                    limit: self.limits.max_part_bytes,
                });
            }
            body.extend_from_slice(&self.buf[..body_end]);
        }
        let finished = self.current_body.take().unwrap();
        let field = self.current_field_name.take().unwrap_or_default();
        out.push(MultipartEvent::PartEnd {
            field_name: field,
            payload: finished.freeze(),
        });

        // Drain through the end of the matched marker.
        let marker_len = if is_closing {
            close_marker.len()
        } else {
            open_marker.len()
        };
        self.buf.drain(..idx + marker_len);

        self.state = if is_closing {
            out.push(MultipartEvent::Complete);
            State::Terminated
        } else {
            State::Headers
        };
        Ok(true)
    }

    // ── Helpers ────────────────────────────────────────────────────

    fn boundary_marker(&self, closing: bool) -> Vec<u8> {
        let mut v = Vec::with_capacity(self.boundary.len() + 6);
        v.extend_from_slice(b"--");
        v.extend_from_slice(&self.boundary);
        if closing {
            v.extend_from_slice(b"--");
        }
        v.extend_from_slice(b"\r\n");
        v
    }
}

// ── Helpers shared with tests ────────────────────────────────────────

/// Extract the `boundary=...` parameter from a `Content-Type` value.
/// Returns `None` when absent or malformed.
pub fn parse_boundary_from_content_type(value: &str) -> Option<String> {
    for part in value.split(';') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("boundary=") {
            // Strip optional quoting.
            let unquoted = rest.trim_matches('"').to_string();
            if !unquoted.is_empty() {
                return Some(unquoted);
            }
        }
    }
    None
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

fn disposition_field_and_file(
    headers: &[(String, String)],
) -> (Option<String>, Option<String>) {
    let Some((_, disp)) = headers
        .iter()
        .find(|(k, _)| k == "content-disposition")
    else {
        return (None, None);
    };
    let mut field_name: Option<String> = None;
    let mut file_name: Option<String> = None;
    for segment in disp.split(';') {
        let segment = segment.trim();
        if let Some(rest) = segment.strip_prefix("name=") {
            field_name = Some(rest.trim_matches('"').to_string());
        } else if let Some(rest) = segment.strip_prefix("filename=") {
            file_name = Some(rest.trim_matches('"').to_string());
        }
    }
    (field_name, file_name)
}

fn kind_for_content_type(ct: Option<&str>) -> BufferKind {
    let Some(ct) = ct else {
        return BufferKind::raw();
    };
    let ct_low = ct.to_ascii_lowercase();
    // Cheap prefix + keyword match. Adopters override on the
    // returned BufferMut if they want a more specific tag.
    if ct_low.starts_with("image/jpeg") {
        BufferKind::jpeg()
    } else if ct_low.starts_with("image/png") {
        BufferKind::png()
    } else if ct_low.starts_with("image/webp") {
        BufferKind::webp()
    } else if ct_low.starts_with("audio/mpeg") {
        BufferKind::mp3()
    } else if ct_low.starts_with("audio/opus") || ct_low.contains("ogg") {
        BufferKind::opus()
    } else if ct_low.starts_with("audio/wav") || ct_low.starts_with("audio/x-wav") {
        BufferKind::wav()
    } else if ct_low.starts_with("video/mp4") {
        BufferKind::mp4()
    } else if ct_low.starts_with("video/webm") {
        BufferKind::webm()
    } else if ct_low.starts_with("application/pdf") {
        BufferKind::pdf()
    } else if ct_low.contains("json") {
        BufferKind::json()
    } else if ct_low.contains("csv") {
        BufferKind::csv()
    } else {
        BufferKind::raw()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(body: &[u8]) -> Vec<u8> {
        body.to_vec()
    }

    #[test]
    fn parses_content_type_boundary() {
        assert_eq!(
            parse_boundary_from_content_type(
                "multipart/form-data; boundary=------abc"
            ),
            Some("------abc".to_string())
        );
        assert_eq!(
            parse_boundary_from_content_type(
                "multipart/form-data; boundary=\"quoted-boundary\""
            ),
            Some("quoted-boundary".to_string())
        );
        assert_eq!(
            parse_boundary_from_content_type("text/plain"),
            None
        );
    }

    #[test]
    fn single_text_part_roundtrip() {
        let body = build(b"\
            --abc\r\n\
            Content-Disposition: form-data; name=\"greeting\"\r\n\
            \r\n\
            hello world\r\n\
            --abc--\r\n");

        let mut p = MultipartParser::new("abc", MultipartLimits::default());
        let events = p.feed(&body).expect("parse");
        assert_eq!(events.len(), 3);
        match &events[0] {
            MultipartEvent::PartStart { field_name, kind, .. } => {
                assert_eq!(field_name, "greeting");
                assert_eq!(kind.slug(), "raw");
            }
            other => panic!("expected PartStart, got {other:?}"),
        }
        match &events[1] {
            MultipartEvent::PartEnd { field_name, payload } => {
                assert_eq!(field_name, "greeting");
                assert_eq!(payload.as_slice(), b"hello world");
            }
            other => panic!("expected PartEnd, got {other:?}"),
        }
        matches!(events[2], MultipartEvent::Complete);
    }

    #[test]
    fn two_parts_with_jpeg_content_type() {
        let body = build(b"\
            --bdy\r\n\
            Content-Disposition: form-data; name=\"field1\"\r\n\
            \r\n\
            value1\r\n\
            --bdy\r\n\
            Content-Disposition: form-data; name=\"image\"; filename=\"a.jpg\"\r\n\
            Content-Type: image/jpeg\r\n\
            \r\n\
            BINARYDATA\r\n\
            --bdy--\r\n");

        let mut p = MultipartParser::new("bdy", MultipartLimits::default());
        let evs = p.feed(&body).unwrap();
        // Two start/end pairs + complete.
        let kinds: Vec<_> = evs
            .iter()
            .filter_map(|e| match e {
                MultipartEvent::PartStart { kind, .. } => Some(kind.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(kinds.len(), 2);
        assert_eq!(kinds[0].slug(), "raw");
        assert_eq!(kinds[1].slug(), "jpeg");

        let payloads: Vec<_> = evs
            .iter()
            .filter_map(|e| match e {
                MultipartEvent::PartEnd { payload, .. } => Some(payload.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(payloads[0].as_slice(), b"value1");
        assert_eq!(payloads[1].as_slice(), b"BINARYDATA");
    }

    #[test]
    fn chunked_feed_works_across_boundary_splits() {
        let body = b"\
            --z\r\n\
            Content-Disposition: form-data; name=\"n\"\r\n\
            \r\n\
            hello world\r\n\
            --z--\r\n";

        let mut p = MultipartParser::new("z", MultipartLimits::default());
        // Feed one byte at a time — the hardest case for streaming
        // parsers.
        let mut events = Vec::new();
        for byte in body {
            events.extend(p.feed(&[*byte]).unwrap());
        }
        let payloads: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                MultipartEvent::PartEnd { payload, .. } => Some(payload.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0].as_slice(), b"hello world");
    }

    #[test]
    fn header_too_large_errors() {
        let mut limits = MultipartLimits::default();
        limits.max_header_bytes = 32;
        let big_header = "Content-Disposition: form-data; name=\"".to_string()
            + &"x".repeat(200)
            + "\"";
        let body = format!(
            "--z\r\n{big_header}\r\n\r\nbody\r\n--z--\r\n"
        );
        let mut p = MultipartParser::new("z", limits);
        let err = p.feed(body.as_bytes()).unwrap_err();
        matches!(err, MultipartError::HeaderTooLarge { .. });
    }

    #[test]
    fn part_too_large_errors() {
        let mut limits = MultipartLimits::default();
        limits.max_part_bytes = 16;
        let big = "x".repeat(1024);
        let body = format!(
            "--z\r\nContent-Disposition: form-data; name=\"n\"\r\n\r\n{big}\r\n--z--\r\n"
        );
        let mut p = MultipartParser::new("z", limits);
        let err = p.feed(body.as_bytes()).unwrap_err();
        matches!(err, MultipartError::PartTooLarge { .. });
    }

    #[test]
    fn nested_multipart_rejected() {
        let body = b"\
            --z\r\n\
            Content-Disposition: form-data; name=\"n\"\r\n\
            Content-Type: multipart/mixed; boundary=inner\r\n\
            \r\n\
            data\r\n\
            --z--\r\n";
        let mut p = MultipartParser::new("z", MultipartLimits::default());
        let err = p.feed(body).unwrap_err();
        matches!(err, MultipartError::Nested);
    }
}
