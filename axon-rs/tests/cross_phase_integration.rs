//! Cross-phase integration tests — v1.4.0.
//!
//! These exercise the ENTIRE v1.4.0 stack in a single flow:
//!
//! WebSocket binary frames (v1.4.0 ingest)
//! → ZeroCopyBuffer pool allocation (v1.4.0)
//! → OTS pipeline synthesis mulaw8 → pcm16 → pcm16_16k (v1.4.0)
//! → Stream<T> with backpressure (v1.4.0)
//! → Trust verification of signed payload (v1.4.0)
//! → ReplayToken emission (v1.4.0)
//! → CognitiveState snapshot (v1.4.0)
//!
//! The point isn't to re-validate each primitive — those have
//! their own test files. It's to assert that the primitives
//! COMPOSE: a flow author combining them all sees bit-identical
//! behaviour to each primitive run in isolation.

use axon::buffer::{BufferKind, ZeroCopyBuffer};
use axon::legal_basis::LegalBasis;
use axon::ots::{global_registry, Pipeline};
use axon::pem::{
    CognitiveState, ContinuityTokenSigner, FixedPoint, InMemoryBackend,
    MemoryEntry, PersistenceBackend,
};
use axon::refinement::TrustProof;
use axon::replay_token::{
    InMemoryReplayLog, ReplayLog, ReplayTokenBuilder, SamplingParams,
};
use axon::stream_effect::parse_backpressure_annotation;
use axon::stream_runtime::Stream as AxonStream;
use axon::trust_verifiers::{verify_hmac_sha256, VerifiedPayload};
use chrono::{Duration, TimeZone, Utc};
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

// ── Composed flow: audio ingest → transcode → snapshot → replay ─────

#[tokio::test]
async fn full_audio_stack_composes_across_every_fase_11_primitive() {
    // 1. SIMULATED INGRESS — WebSocket binary frames carrying a
    //    signed μ-law 8 kHz audio chunk. The "signature" is HMAC
    //    over the chunk; verification produces a Trusted<Bytes>.
    let key = b"webhook-signer-key";
    let audio_payload: Vec<u8> = (0u8..200).map(|i| i.wrapping_mul(3)).collect();
    let mut mac = HmacSha256::new_from_slice(key).unwrap();
    mac.update(&audio_payload);
    let tag = mac.finalize().into_bytes();

    let verified: VerifiedPayload =
        verify_hmac_sha256(&audio_payload, &tag, key, "webhook-v1")
            .expect("HMAC verify");
    assert_eq!(verified.proof, TrustProof::Hmac);

    // 2. ZEROCOPY BUFFER — the trusted bytes land in a buffer
    //    tagged with the owning tenant.
    let mulaw_buf = ZeroCopyBuffer::from_bytes(
        audio_payload.clone(),
        BufferKind::mulaw8(),
    )
    .with_tenant("alpha");
    assert_eq!(mulaw_buf.kind().slug(), "mulaw8");

    // 3. OTS PIPELINE — auto-synthesis mulaw8 → pcm16.
    let registry = global_registry();
    let pipeline = Pipeline::from_registry(
        registry,
        &BufferKind::mulaw8(),
        &BufferKind::pcm16(),
    )
    .expect("mulaw8 → pcm16 path exists");
    assert!(!pipeline.crosses_process_boundary());
    let pcm_buf = pipeline.execute(&mulaw_buf).expect("transcode");
    assert_eq!(pcm_buf.kind().slug(), "pcm16");
    assert_eq!(pcm_buf.tenant_id(), Some("alpha"));
    assert_eq!(
        pcm_buf.len(),
        mulaw_buf.len() * 2,
        "PCM16 is 2× wider than μ-law"
    );

    // 4. STREAM<T> — the PCM buffer enters a backpressure-enabled
    //    stream feeding a (simulated) transcriber.
    let ann =
        parse_backpressure_annotation("drop_oldest").expect("valid policy");
    let stream: AxonStream<ZeroCopyBuffer> = AxonStream::new(4, ann);
    stream.push(pcm_buf.clone()).await.expect("push");
    let from_stream = stream.pop().await.expect("pop");
    assert_eq!(from_stream.kind(), pcm_buf.kind());
    // Arc<StreamMetrics> derefs to StreamMetrics, so .snapshot() reads
    // the live atomic counters directly. The previous try_unwrap dance
    // always failed (clone bumped the count to 2) and fell back to a
    // default-zeroed StreamMetrics, masking the actual increment with
    // a snapshot of zeros — bug pre-existed since v1.8.0+.
    let snap = stream.metrics.snapshot();
    assert_eq!(snap.items_pushed, 1);
    assert_eq!(snap.items_delivered, 1);

    // 5. REPLAY TOKEN — the transcription effect emits a token
    //    carrying the canonical-hashed inputs + outputs.
    let replay_log = InMemoryReplayLog::new();
    let token = ReplayTokenBuilder::new()
        .effect_name("llm_infer:whisper")
        .inputs(json!({
            "flow_id": "transcribe-flow-1",
            "audio_sha256": hex_string(&verified_sha256(&from_stream)),
        }))
        .outputs(json!({"transcript": "hello world"}))
        .model_version("openai/whisper-large-v3")
        .sampling(SamplingParams {
            temperature: Some(0.0),
            seed: Some(42),
            ..Default::default()
        })
        .timestamp(Utc.with_ymd_and_hms(2026, 4, 22, 12, 0, 0).unwrap())
        .nonce([5u8; 16])
        .mint();
    replay_log.append(token.clone()).await.expect("append");

    let fetched = replay_log
        .get(&token.token_hash_hex)
        .await
        .expect("roundtrip");
    assert_eq!(fetched.token_hash_hex, token.token_hash_hex);

    // 6. COGNITIVE STATE — the flow snapshots its posture mid-
    //    conversation. On reconnect, the state rehydrates bit-
    //    identical.
    let pem_backend = InMemoryBackend::new();
    let mut state = CognitiveState::new(
        "sess-integration-1",
        "alpha",
        "transcribe-flow-1",
    );
    state.created_at = Utc.with_ymd_and_hms(2026, 4, 22, 12, 0, 0).unwrap();
    state.last_updated_at = state.created_at;
    state.subject_user_id = Some("usr-42".to_string());
    state.density_matrix = vec![FixedPoint::vec_from_f64(&[0.2, 0.8])];
    state.belief_state = json!({
        "last_transcript_confidence": 0.92,
        "transcribed_so_far": 1,
    });
    state.short_term_memory.push(MemoryEntry {
        key: "last_replay_token".into(),
        payload: json!({"hash": token.token_hash_hex}),
        symbolic_refs: vec![],
        stored_at: state.created_at,
    });

    pem_backend
        .persist(&state.session_id, &state, Duration::minutes(15))
        .await
        .expect("persist");

    // 7. CONTINUITY TOKEN — the client reconnects; server verifies
    //    + rehydrates. End-to-end round-trip — no drift.
    let signer = ContinuityTokenSigner::new([11u8; 32]);
    let wire = signer.sign(&axon::pem::ContinuityToken::new(
        state.session_id.clone(),
        Duration::minutes(15),
    ));
    let parsed = signer.verify(&wire).expect("verify");
    let rehydrated = pem_backend
        .restore(&parsed.session_id)
        .await
        .expect("restore");
    assert_eq!(rehydrated.density_matrix, state.density_matrix);
    assert_eq!(rehydrated.short_term_memory, state.short_term_memory);
    assert_eq!(
        rehydrated.belief_state.get("last_transcript_confidence"),
        state.belief_state.get("last_transcript_confidence")
    );
}

// ── Legal-basis composition ────────────────────────────────────────

#[test]
fn legal_basis_slugs_route_to_regulations() {
    // Every variant from v1.4.0 maps to exactly one regulation
    // family — the composition invariant the threat model relies on.
    for basis in LegalBasis::ALL {
        let reg = basis.regulation();
        let slug = basis.slug();
        assert!(
            slug.starts_with(reg.slug()),
            "{slug} should begin with its regulation family {:?}",
            reg.slug()
        );
    }
}

// ── Helpers that the primitives already expose but Rust's test
//    wiring needs locally here for convenience ─────────────────────

fn hex_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn verified_sha256(buf: &ZeroCopyBuffer) -> [u8; 32] {
    buf.sha256()
}

// (Removed `StreamMetricsCompat::into_from` no-op shim — it returned a
// default-zeroed StreamMetrics, which silently masked the actual
// items_pushed/items_delivered values. The test now snapshots the live
// `Arc<StreamMetrics>` directly via auto-deref.)
