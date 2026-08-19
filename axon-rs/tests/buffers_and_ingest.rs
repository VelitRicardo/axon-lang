//! Integration tests for v1.4.0 — Zero-Copy Multimodal
//! Buffers.
//!
//! Covers end-to-end flows that exercise buffer + pool + ingest
//! together; module-level unit tests live inside each source file.

use axon::buffer::{
    BufferKind, BufferKindRegistry, BufferMut, BufferPool, PoolClass,
    ZeroCopyBuffer,
};
use axon::ingest::multipart::{
    parse_boundary_from_content_type, MultipartEvent, MultipartLimits,
    MultipartParser,
};
use axon::ingest::ws_binary::{WsBinaryAccumulator, WsBinaryLimits};

// ── End-to-end: multipart flows into ZeroCopyBuffer ────────────────

#[test]
fn multipart_upload_produces_zero_copy_buffers_per_field() {
    let body = b"\
        --bdy\r\n\
        Content-Disposition: form-data; name=\"caption\"\r\n\
        \r\n\
        a photo of my dog\r\n\
        --bdy\r\n\
        Content-Disposition: form-data; name=\"image\"; filename=\"dog.jpg\"\r\n\
        Content-Type: image/jpeg\r\n\
        \r\n\
        BINARYPAYLOAD_OF_JPEG\r\n\
        --bdy--\r\n";

    let mut parser =
        MultipartParser::new("bdy", MultipartLimits::default());
    let events = parser.feed(body).expect("parse ok");

    let payloads: Vec<(String, ZeroCopyBuffer)> = events
        .into_iter()
        .filter_map(|e| match e {
            MultipartEvent::PartEnd {
                field_name,
                payload,
            } => Some((field_name, payload)),
            _ => None,
        })
        .collect();

    assert_eq!(payloads.len(), 2);
    assert_eq!(payloads[0].0, "caption");
    assert_eq!(payloads[0].1.kind().slug(), "raw");
    assert_eq!(payloads[0].1.as_slice(), b"a photo of my dog");

    assert_eq!(payloads[1].0, "image");
    assert_eq!(payloads[1].1.kind().slug(), "jpeg");
    assert_eq!(payloads[1].1.as_slice(), b"BINARYPAYLOAD_OF_JPEG");
}

#[test]
fn boundary_extracted_from_content_type_header() {
    let ct = "multipart/form-data; boundary=----xyz";
    assert_eq!(
        parse_boundary_from_content_type(ct).as_deref(),
        Some("----xyz")
    );
}

// ── End-to-end: fragmented WS binary into single buffer ────────────

#[test]
fn websocket_fragments_stitch_into_one_buffer() {
    let mut acc = WsBinaryAccumulator::new(
        BufferKind::pcm16(),
        WsBinaryLimits::default(),
    )
    .with_tenant("alpha");

    // Three fragments, final FIN.
    assert!(acc.feed(0x2, false, b"audio").unwrap().is_none());
    assert!(acc.feed(0x0, false, b"-frame").unwrap().is_none());
    let out = acc
        .feed(0x0, true, b"-final")
        .unwrap()
        .expect("final buffer");

    assert_eq!(out.as_slice(), b"audio-frame-final");
    assert_eq!(out.kind().slug(), "pcm16");
    assert_eq!(out.tenant_id(), Some("alpha"));
}

// ── Pool reuse under steady-state traffic ───────────────────────────

#[test]
fn pool_reuses_slabs_for_repeated_same_size_allocations() {
    let pool = BufferPool::default();
    for _ in 0..10 {
        let (slab, class) = pool.acquire(500);
        pool.release(slab, class);
    }
    let snap = pool.snapshot();
    // First acquire is a miss; the next nine are hits.
    assert_eq!(snap.pool_misses[&PoolClass::Small], 1);
    assert_eq!(snap.pool_hits[&PoolClass::Small], 9);
}

#[test]
fn pool_oversize_path_bypasses_class_cache() {
    let pool = BufferPool::default();
    let huge = 20 * 1024 * 1024;
    for _ in 0..3 {
        let (slab, class) = pool.acquire(huge);
        pool.release(slab, class);
    }
    let snap = pool.snapshot();
    assert_eq!(snap.oversize_allocations_total, 3);
    // Oversize path doesn't count as hits/misses on pooled classes.
    assert_eq!(snap.pool_hits[&PoolClass::Huge], 0);
    assert_eq!(snap.pool_misses[&PoolClass::Huge], 0);
}

// ── Kind registry open-extensibility regression ─────────────────────

#[test]
fn registry_accepts_adopter_custom_kind() {
    let custom = BufferKind::new("siemens_dicom");
    assert_eq!(custom.slug(), "siemens_dicom");

    let again = BufferKind::new("siemens_dicom");
    assert_eq!(custom, again);

    assert!(BufferKindRegistry::global()
        .known_slugs()
        .contains(&"siemens_dicom".to_string()));
}

// ── Slicing + fan-out semantics ─────────────────────────────────────

#[test]
fn slice_of_slice_computes_correct_range() {
    let data = (0u8..100).collect::<Vec<u8>>();
    let buf = ZeroCopyBuffer::from_bytes(data, BufferKind::raw());
    let mid = buf.slice(10..60);
    let inner = mid.slice(5..15);
    assert_eq!(inner.as_slice(), &(15u8..25).collect::<Vec<u8>>()[..]);
    // All three views share the same carrier.
    assert_eq!(buf.sharers(), 3);
}

#[test]
fn buffer_mut_freeze_then_slice_preserves_bytes() {
    let mut bm = BufferMut::with_capacity(1024, BufferKind::raw())
        .with_tenant("alpha");
    for chunk in [b"hello ", b"world!"] {
        bm.extend_from_slice(chunk);
    }
    let frozen = bm.freeze();
    let mid = frozen.slice(6..11);
    assert_eq!(mid.as_slice(), b"world");
    assert_eq!(mid.tenant_id(), Some("alpha"));
}
