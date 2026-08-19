//! Zero-copy multimodal buffers — v1.4.0.
//!
//! Bytes that enter the runtime through a network socket, file
//! handle or FFI boundary land **directly** in a region of Rust-
//! owned memory. The Python layer manipulates `SymbolicPtr<T>`
//! handles (Arc clones) — never the raw bytes — until the final
//! consumer (transcoder, compressor, sink) asks for a slice.
//!
//! Three building blocks:
//!
//! - [`BufferKind`] — content-kind tag (`raw`, `pcm16`, `mulaw8`,
//!   `jpeg`, …). Extensible (unlike the closed catalogues of 11.a);
//!   adopters can register new kinds via [`BufferKind::register`].
//! - [`ZeroCopyBuffer`] — the primitive. An `Arc<[u8]>` plus a
//!   [`BufferKind`] tag. Clone is O(1) (Arc refcount bump); slicing
//!   returns another `ZeroCopyBuffer` that shares the same backing
//!   allocation.
//! - [`BufferMut`] — mutable in-flight builder. Accumulates bytes
//!   during ingest then `.freeze()`s into an immutable
//!   `ZeroCopyBuffer` at end-of-stream.
//!
//! Pool-backed allocation lives in the sibling [`pool`] module.

pub mod kind;
pub mod pool;

use std::sync::Arc;

pub use self::kind::{BufferKind, BufferKindRegistry};
pub use self::pool::{BufferPool, BufferPoolSnapshot, PoolClass};

// ── ZeroCopyBuffer ────────────────────────────────────────────────────

/// An immutable view over a region of bytes. Cloneable at O(1) — the
/// backing storage is an `Arc<[u8]>` so clones are refcount bumps.
///
/// Slicing returns another `ZeroCopyBuffer` that references the same
/// underlying allocation: no copies, no new allocations on the hot
/// path. When the last `ZeroCopyBuffer` referencing an allocation
/// drops, the storage returns to the [`BufferPool`] (if it came from
/// one) or is freed (if direct-allocated).
#[derive(Debug, Clone)]
pub struct ZeroCopyBuffer {
    /// Backing storage. `Arc` gives us cheap clones + automatic
    /// release when all views drop.
    storage: Arc<[u8]>,
    /// Sub-range within `storage` that this view exposes. Invariant:
    /// `start <= end <= storage.len()`.
    start: usize,
    end: usize,
    /// Content-kind tag. `raw` for untagged bytes; adopters upgrade
    /// to domain-specific kinds at the point where they know (e.g.
    /// after parsing a multipart header, upgrade `raw` → `jpeg`).
    kind: BufferKind,
    /// Opaque tenant slug for pool bookkeeping. `None` when the
    /// buffer was direct-allocated outside a tenant context.
    tenant_id: Option<Arc<str>>,
}

impl ZeroCopyBuffer {
    /// Construct from an already-owned byte slice. Copies once into
    /// the Arc — use [`ZeroCopyBuffer::from_arc`] to avoid that copy
    /// when the caller already has an `Arc<[u8]>`.
    pub fn from_bytes(bytes: impl Into<Vec<u8>>, kind: BufferKind) -> Self {
        let v = bytes.into();
        let len = v.len();
        let storage: Arc<[u8]> = v.into();
        ZeroCopyBuffer {
            storage,
            start: 0,
            end: len,
            kind,
            tenant_id: None,
        }
    }

    /// Construct from an existing `Arc<[u8]>` without copying.
    pub fn from_arc(storage: Arc<[u8]>, kind: BufferKind) -> Self {
        let len = storage.len();
        ZeroCopyBuffer {
            storage,
            start: 0,
            end: len,
            kind,
            tenant_id: None,
        }
    }

    /// Tag the buffer with its owning tenant (for pool accounting).
    pub fn with_tenant(mut self, tenant_id: impl Into<Arc<str>>) -> Self {
        self.tenant_id = Some(tenant_id.into());
        self
    }

    /// The buffer's visible length in bytes.
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Content-kind tag.
    pub fn kind(&self) -> BufferKind {
        self.kind.clone()
    }

    pub fn tenant_id(&self) -> Option<&str> {
        self.tenant_id.as_deref()
    }

    /// Upgrade the content-kind tag (e.g. `raw` → `jpeg` after
    /// format detection). Returns a new `ZeroCopyBuffer` that shares
    /// the backing storage; the original view is untouched so other
    /// holders see the old kind unchanged.
    pub fn retag(&self, kind: BufferKind) -> Self {
        let mut cloned = self.clone();
        cloned.kind = kind;
        cloned
    }

    /// Borrow the visible byte range as a slice. Callers usually
    /// reach for [`ZeroCopyBuffer::as_slice`] (alias) or feed this
    /// straight to their consumer. **Do not** copy the slice on the
    /// hot path — pass the `ZeroCopyBuffer` by reference instead.
    pub fn as_slice(&self) -> &[u8] {
        &self.storage[self.start..self.end]
    }

    /// Return a sub-view into this buffer. O(1) — no copy; the
    /// returned buffer shares the same `Arc`.
    pub fn slice(&self, range: std::ops::Range<usize>) -> Self {
        let len = self.len();
        assert!(
            range.start <= range.end,
            "slice start {} > end {}",
            range.start,
            range.end
        );
        assert!(
            range.end <= len,
            "slice end {} exceeds buffer len {}",
            range.end,
            len
        );
        ZeroCopyBuffer {
            storage: Arc::clone(&self.storage),
            start: self.start + range.start,
            end: self.start + range.end,
            kind: self.kind.clone(),
            tenant_id: self.tenant_id.clone(),
        }
    }

    /// Number of live views over the backing storage (Arc strong
    /// count). Useful for observability; do NOT use for flow control.
    pub fn sharers(&self) -> usize {
        Arc::strong_count(&self.storage)
    }

    /// Compute a SHA-256 over the visible slice. Not cached — callers
    /// that need repeated hashes should wrap in their own cache.
    pub fn sha256(&self) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(self.as_slice());
        let out = h.finalize();
        let mut array = [0u8; 32];
        array.copy_from_slice(&out);
        array
    }
}

// ── BufferMut (in-flight builder) ────────────────────────────────────

/// Mutable append-only builder used while bytes are still arriving.
/// When the ingest stream terminates, call [`BufferMut::freeze`] to
/// convert into an immutable [`ZeroCopyBuffer`] with a single Arc
/// construction (no copy).
#[derive(Debug)]
pub struct BufferMut {
    storage: Vec<u8>,
    kind: BufferKind,
    tenant_id: Option<Arc<str>>,
}

impl BufferMut {
    /// Build with initial reserved capacity.
    pub fn with_capacity(capacity: usize, kind: BufferKind) -> Self {
        BufferMut {
            storage: Vec::with_capacity(capacity),
            kind,
            tenant_id: None,
        }
    }

    pub fn with_tenant(mut self, tenant_id: impl Into<Arc<str>>) -> Self {
        self.tenant_id = Some(tenant_id.into());
        self
    }

    pub fn len(&self) -> usize {
        self.storage.len()
    }

    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.storage.capacity()
    }

    /// Append bytes. Grows the internal `Vec` using the standard
    /// doubling policy.
    pub fn extend_from_slice(&mut self, bytes: &[u8]) {
        self.storage.extend_from_slice(bytes);
    }

    /// Freeze into an immutable `ZeroCopyBuffer`. This is the moment
    /// the storage transitions from `Vec<u8>` (unique mutable) to
    /// `Arc<[u8]>` (shared immutable). The `Vec -> Arc<[u8]>`
    /// conversion reuses the allocation when possible.
    pub fn freeze(self) -> ZeroCopyBuffer {
        let len = self.storage.len();
        let storage: Arc<[u8]> = self.storage.into();
        ZeroCopyBuffer {
            storage,
            start: 0,
            end: len,
            kind: self.kind,
            tenant_id: self.tenant_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_bytes_roundtrip() {
        let b = ZeroCopyBuffer::from_bytes(vec![1, 2, 3], BufferKind::raw());
        assert_eq!(b.len(), 3);
        assert_eq!(b.as_slice(), &[1, 2, 3]);
        assert_eq!(b.kind(), BufferKind::raw());
    }

    #[test]
    fn clone_shares_storage() {
        let b = ZeroCopyBuffer::from_bytes(vec![0u8; 1024], BufferKind::raw());
        let c = b.clone();
        assert_eq!(b.sharers(), 2);
        assert_eq!(c.sharers(), 2);
        drop(c);
        assert_eq!(b.sharers(), 1);
    }

    #[test]
    fn slice_shares_storage_and_preserves_kind() {
        let b = ZeroCopyBuffer::from_bytes(
            vec![10, 20, 30, 40],
            BufferKind::pcm16(),
        );
        let s = b.slice(1..3);
        assert_eq!(s.as_slice(), &[20, 30]);
        assert_eq!(s.kind(), BufferKind::pcm16());
        assert_eq!(b.sharers(), 2);
    }

    #[test]
    fn slice_out_of_range_panics() {
        let b = ZeroCopyBuffer::from_bytes(vec![1, 2, 3], BufferKind::raw());
        let result = std::panic::catch_unwind(|| {
            let _ = b.slice(0..10);
        });
        assert!(result.is_err());
    }

    #[test]
    fn retag_leaves_original_kind() {
        let b = ZeroCopyBuffer::from_bytes(vec![1, 2, 3], BufferKind::raw());
        let j = b.retag(BufferKind::jpeg());
        assert_eq!(b.kind(), BufferKind::raw());
        assert_eq!(j.kind(), BufferKind::jpeg());
        // Storage is still shared.
        assert!(b.sharers() >= 2);
    }

    #[test]
    fn buffer_mut_freeze_reuses_allocation() {
        let mut bm = BufferMut::with_capacity(1024, BufferKind::raw());
        bm.extend_from_slice(b"hello ");
        bm.extend_from_slice(b"world");
        let frozen = bm.freeze();
        assert_eq!(frozen.as_slice(), b"hello world");
        assert_eq!(frozen.len(), 11);
    }

    #[test]
    fn sha256_computes_on_visible_slice_only() {
        let b = ZeroCopyBuffer::from_bytes(
            vec![b'a', b'b', b'c', b'd'],
            BufferKind::raw(),
        );
        let s = b.slice(1..3); // "bc"
        let reference = ZeroCopyBuffer::from_bytes(
            b"bc".to_vec(),
            BufferKind::raw(),
        );
        assert_eq!(s.sha256(), reference.sha256());
    }

    #[test]
    fn tenant_tag_propagates_through_clone_and_slice() {
        let b = ZeroCopyBuffer::from_bytes(
            vec![0u8; 16],
            BufferKind::raw(),
        )
        .with_tenant("alpha");
        let c = b.clone();
        assert_eq!(c.tenant_id(), Some("alpha"));
        let s = b.slice(0..4);
        assert_eq!(s.tenant_id(), Some("alpha"));
    }
}
