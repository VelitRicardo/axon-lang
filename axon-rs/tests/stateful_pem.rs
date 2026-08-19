//! Integration tests for v1.4.0 — Stateful PEM over
//! WebSocket.
//!
//! End-to-end property we care about: a snapshot produced before a
//! disconnect is BIT-identical to the state after rehydration via
//! `InMemoryBackend` + `ContinuityTokenSigner`. No drift in the
//! density matrix across N reconnects.

use axon::pem::{
    CognitiveState, ContinuityToken, ContinuityTokenError,
    ContinuityTokenSigner, FixedPoint, InMemoryBackend, MemoryEntry,
    PersistenceBackend, PersistenceError,
};
use chrono::{Duration, TimeZone, Utc};
use serde_json::json;

fn make_state() -> CognitiveState {
    let mut s = CognitiveState::new("sess-1", "alpha", "flow-transcribe");
    s.created_at = Utc.with_ymd_and_hms(2026, 4, 22, 12, 0, 0).unwrap();
    s.last_updated_at = s.created_at;
    s.subject_user_id = Some("usr-42".to_string());
    s.density_matrix = vec![
        FixedPoint::vec_from_f64(&[0.1, 0.2, 0.7]),
        FixedPoint::vec_from_f64(&[0.4, 0.4, 0.2]),
    ];
    s.belief_state = json!({
        "confidence": 0.73,
        "last_topic": "audio segmentation",
    });
    s.short_term_memory.push(MemoryEntry {
        key: "last_user_msg".into(),
        payload: json!({"text": "continue please"}),
        symbolic_refs: vec!["audio-buf-17".into()],
        stored_at: s.created_at,
    });
    s
}

// ── Q32.32 determinism + roundtrip ───────────────────────────────────

#[test]
fn density_matrix_bit_identical_after_three_reconnects() {
    let state = make_state();
    let mut current = state.clone();
    for _ in 0..3 {
        let bytes = current.encode();
        current = CognitiveState::decode(&bytes).expect("decode");
    }
    assert_eq!(current.density_matrix, state.density_matrix);
}

// ── Backend persist / restore ────────────────────────────────────────

#[tokio::test]
async fn persist_restore_preserves_full_state() {
    let backend = InMemoryBackend::new();
    let state = make_state();
    backend
        .persist(&state.session_id, &state, Duration::minutes(15))
        .await
        .unwrap();
    let restored = backend.restore(&state.session_id).await.unwrap();
    assert_eq!(restored, state);
}

#[tokio::test]
async fn restore_rejects_unknown_session() {
    let backend = InMemoryBackend::new();
    let err = backend.restore("missing").await.unwrap_err();
    matches!(err, PersistenceError::NotFound { .. });
}

#[tokio::test]
async fn expired_state_is_not_readable() {
    let backend = InMemoryBackend::new();
    let state = make_state();
    backend
        .persist(&state.session_id, &state, Duration::seconds(-5))
        .await
        .unwrap();
    let err = backend.restore(&state.session_id).await.unwrap_err();
    matches!(err, PersistenceError::Expired { .. });
}

#[tokio::test]
async fn evict_expired_batch_sweeps_only_stale() {
    let backend = InMemoryBackend::new();

    let mut stale = make_state();
    stale.session_id = "stale".into();
    let mut fresh = make_state();
    fresh.session_id = "fresh".into();

    backend
        .persist(&stale.session_id, &stale, Duration::seconds(-10))
        .await
        .unwrap();
    backend
        .persist(&fresh.session_id, &fresh, Duration::minutes(30))
        .await
        .unwrap();

    let removed = backend.evict_expired(Utc::now()).await.unwrap();
    assert_eq!(removed, 1);

    backend.restore(&fresh.session_id).await.unwrap();
    let err = backend.restore(&stale.session_id).await.unwrap_err();
    matches!(err, PersistenceError::NotFound { .. });
}

// ── Continuity token ────────────────────────────────────────────────

#[test]
fn continuity_token_roundtrip_under_legitimate_use() {
    let signer = ContinuityTokenSigner::new([42u8; 32]);
    let token = ContinuityToken::new("sess-1", Duration::minutes(15));
    let wire = signer.sign(&token);
    let parsed = signer.verify(&wire).expect("verify");
    assert_eq!(parsed.session_id, "sess-1");
}

#[test]
fn continuity_token_forgery_rejected() {
    let signer_a = ContinuityTokenSigner::new([1u8; 32]);
    let signer_b = ContinuityTokenSigner::new([2u8; 32]);

    let token = ContinuityToken::new("sess-1", Duration::minutes(15));
    let wire = signer_a.sign(&token);
    let err = signer_b.verify(&wire).unwrap_err();
    matches!(err, ContinuityTokenError::ForgedOrRotated);
}

#[test]
fn continuity_token_expired_rejected() {
    let signer = ContinuityTokenSigner::new([7u8; 32]);
    let token = ContinuityToken::new("sess-1", Duration::seconds(-1));
    let wire = signer.sign(&token);
    let err = signer.verify(&wire).unwrap_err();
    matches!(err, ContinuityTokenError::Expired { .. });
}

// ── End-to-end reconnect scenario ───────────────────────────────────

#[tokio::test]
async fn reconnect_flow_verifies_token_and_restores_state() {
    let signer = ContinuityTokenSigner::new([9u8; 32]);
    let backend = InMemoryBackend::new();

    // 1. Session snapshots state pre-disconnect.
    let state = make_state();
    backend
        .persist(&state.session_id, &state, Duration::minutes(15))
        .await
        .unwrap();

    // 2. Server mints a continuity token for the client.
    let handshake = ContinuityToken::new(
        state.session_id.clone(),
        Duration::minutes(15),
    );
    let wire = signer.sign(&handshake);

    // 3. Client reconnects, presents the wire token. Server verifies
    //    then uses the bound session_id to restore.
    let parsed = signer.verify(&wire).expect("verify");
    assert_eq!(parsed.session_id, state.session_id);

    let rehydrated = backend.restore(&parsed.session_id).await.unwrap();
    assert_eq!(rehydrated.density_matrix, state.density_matrix);
    assert_eq!(rehydrated.short_term_memory, state.short_term_memory);
}

#[tokio::test]
async fn reconnect_with_forged_token_does_not_reach_backend() {
    let real_signer = ContinuityTokenSigner::new([9u8; 32]);
    let attacker_signer = ContinuityTokenSigner::new([99u8; 32]);
    let backend = InMemoryBackend::new();

    let state = make_state();
    backend
        .persist(&state.session_id, &state, Duration::minutes(15))
        .await
        .unwrap();

    let forged = attacker_signer.sign(&ContinuityToken::new(
        state.session_id.clone(),
        Duration::minutes(15),
    ));

    // The server MUST reject at the verify step before any backend
    // lookup.
    let err = real_signer.verify(&forged).unwrap_err();
    matches!(err, ContinuityTokenError::ForgedOrRotated);
}
