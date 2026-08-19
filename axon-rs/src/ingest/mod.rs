//! Network ingest paths that deposit bytes directly into
//! [`crate::buffer::ZeroCopyBuffer`]s.
//!
//! v1.4.0. Two primary sources today:
//!
//! - [`multipart`] — `multipart/form-data` from HTTP uploads, parsed
//!   field-by-field into per-field `BufferMut`s that freeze at end
//!   of part.
//! - [`ws_binary`] — WebSocket binary frame accumulator. Fragmented
//!   frames are stitched into a single contiguous buffer.

pub mod multipart;
pub mod ws_binary;
