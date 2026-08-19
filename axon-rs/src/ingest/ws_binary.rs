//! WebSocket binary-frame accumulator.
//!
//! v1.4.0. WebSocket spec (RFC 6455) allows a message to
//! be split across multiple frames:
//!
//! - Frame #1: opcode=0x2 (binary), FIN=0
//! - Frame #2..N: opcode=0x0 (continuation), FIN=0
//! - Final frame: opcode=0x0 (continuation), FIN=1
//!
//! The accumulator stitches fragments into a single contiguous
//! [`crate::buffer::ZeroCopyBuffer`] without copying the payload
//! across frames — bytes land directly in a [`BufferMut`] and
//! freeze when FIN arrives.
//!
//! The accumulator is frame-shape-agnostic (it doesn't parse the WS
//! frame header; the transport layer does that). Callers hand us
//! already-unmasked payload bytes plus the `is_final` flag.

use crate::buffer::{BufferKind, BufferMut, ZeroCopyBuffer};

#[derive(Debug)]
pub enum WsBinaryError {
    /// A continuation frame arrived without a preceding non-FIN
    /// binary opener.
    OrphanContinuation,
    /// Accumulated payload exceeded the configured ceiling.
    MessageTooLarge {
        limit: usize,
    },
}

impl std::fmt::Display for WsBinaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OrphanContinuation => {
                write!(f, "continuation frame without opener")
            }
            Self::MessageTooLarge { limit } => {
                write!(f, "binary message exceeded {limit} bytes")
            }
        }
    }
}

impl std::error::Error for WsBinaryError {}

/// Per-accumulator configuration.
#[derive(Debug, Clone)]
pub struct WsBinaryLimits {
    pub max_message_bytes: usize,
}

impl Default for WsBinaryLimits {
    fn default() -> Self {
        // 128 MiB default — large enough for a short video clip,
        // small enough to catch runaway producers. Adopters override
        // for long-lived ingest of multi-GiB streams.
        WsBinaryLimits {
            max_message_bytes: 128 * 1024 * 1024,
        }
    }
}

/// Per-connection accumulator. Consume it across frames of a single
/// connection; reset via [`WsBinaryAccumulator::reset`] if the
/// connection cycles.
pub struct WsBinaryAccumulator {
    buffer: Option<BufferMut>,
    kind: BufferKind,
    limits: WsBinaryLimits,
    tenant_id: Option<String>,
}

impl WsBinaryAccumulator {
    pub fn new(kind: BufferKind, limits: WsBinaryLimits) -> Self {
        WsBinaryAccumulator {
            buffer: None,
            kind,
            limits,
            tenant_id: None,
        }
    }

    pub fn with_tenant(mut self, tenant_id: impl Into<String>) -> Self {
        self.tenant_id = Some(tenant_id.into());
        self
    }

    /// Start (or continue) a message. `opcode` is 0x2 for a fresh
    /// binary message and 0x0 for a continuation. `is_final` is the
    /// FIN flag from the frame header.
    ///
    /// Returns `Some(buffer)` at end-of-message, `None` while
    /// accumulating.
    pub fn feed(
        &mut self,
        opcode: u8,
        is_final: bool,
        payload: &[u8],
    ) -> Result<Option<ZeroCopyBuffer>, WsBinaryError> {
        match opcode {
            0x2 => {
                // Opener. Any previous incomplete buffer is
                // overwritten — the spec forbids interleaved
                // messages but we tolerate it by discarding the
                // partial.
                let mut body = BufferMut::with_capacity(
                    payload.len().max(4 * 1024),
                    self.kind.clone(),
                );
                if let Some(tenant) = &self.tenant_id {
                    body = body.with_tenant(tenant.as_str());
                }
                if payload.len() > self.limits.max_message_bytes {
                    return Err(WsBinaryError::MessageTooLarge {
                        limit: self.limits.max_message_bytes,
                    });
                }
                body.extend_from_slice(payload);
                self.buffer = Some(body);
            }
            0x0 => {
                // Continuation.
                let Some(buf) = self.buffer.as_mut() else {
                    return Err(WsBinaryError::OrphanContinuation);
                };
                if buf.len() + payload.len() > self.limits.max_message_bytes {
                    return Err(WsBinaryError::MessageTooLarge {
                        limit: self.limits.max_message_bytes,
                    });
                }
                buf.extend_from_slice(payload);
            }
            _ => {
                // Any other opcode (text, control) is out of scope
                // for this accumulator — the transport handles it.
                return Ok(None);
            }
        }

        if is_final {
            let body = self.buffer.take().expect("buffer present in final frame");
            Ok(Some(body.freeze()))
        } else {
            Ok(None)
        }
    }

    pub fn reset(&mut self) {
        self.buffer = None;
    }

    /// Observability — bytes accumulated so far (0 when idle).
    pub fn pending_bytes(&self) -> usize {
        self.buffer.as_ref().map(|b| b.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_frame_message() {
        let mut acc = WsBinaryAccumulator::new(
            BufferKind::pcm16(),
            WsBinaryLimits::default(),
        );
        let result = acc
            .feed(0x2, true, b"hello")
            .unwrap()
            .expect("buffer");
        assert_eq!(result.as_slice(), b"hello");
        assert_eq!(result.kind().slug(), "pcm16");
    }

    #[test]
    fn fragmented_message_is_stitched() {
        let mut acc = WsBinaryAccumulator::new(
            BufferKind::raw(),
            WsBinaryLimits::default(),
        );
        assert!(acc.feed(0x2, false, b"he").unwrap().is_none());
        assert!(acc.feed(0x0, false, b"ll").unwrap().is_none());
        let end = acc
            .feed(0x0, true, b"o")
            .unwrap()
            .expect("buffer on FIN");
        assert_eq!(end.as_slice(), b"hello");
    }

    #[test]
    fn orphan_continuation_errors() {
        let mut acc = WsBinaryAccumulator::new(
            BufferKind::raw(),
            WsBinaryLimits::default(),
        );
        let err = acc.feed(0x0, true, b"x").unwrap_err();
        matches!(err, WsBinaryError::OrphanContinuation);
    }

    #[test]
    fn message_too_large_errors() {
        let mut acc = WsBinaryAccumulator::new(
            BufferKind::raw(),
            WsBinaryLimits {
                max_message_bytes: 4,
            },
        );
        let err = acc.feed(0x2, true, b"too-big").unwrap_err();
        matches!(err, WsBinaryError::MessageTooLarge { .. });
    }

    #[test]
    fn partial_then_oversize_errors_on_second_frame() {
        let mut acc = WsBinaryAccumulator::new(
            BufferKind::raw(),
            WsBinaryLimits {
                max_message_bytes: 4,
            },
        );
        assert!(acc.feed(0x2, false, b"ok").unwrap().is_none());
        let err = acc.feed(0x0, true, b"xxxx").unwrap_err();
        matches!(err, WsBinaryError::MessageTooLarge { .. });
    }

    #[test]
    fn tenant_tag_propagates_into_buffer() {
        let mut acc = WsBinaryAccumulator::new(
            BufferKind::raw(),
            WsBinaryLimits::default(),
        )
        .with_tenant("alpha");
        let out = acc
            .feed(0x2, true, b"payload")
            .unwrap()
            .expect("buffer");
        assert_eq!(out.tenant_id(), Some("alpha"));
    }

    #[test]
    fn control_opcode_is_ignored() {
        let mut acc = WsBinaryAccumulator::new(
            BufferKind::raw(),
            WsBinaryLimits::default(),
        );
        // 0x9 = ping — not our concern.
        assert!(acc.feed(0x9, true, b"ping").unwrap().is_none());
    }
}
