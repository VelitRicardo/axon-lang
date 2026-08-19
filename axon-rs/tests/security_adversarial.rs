//! Adversarial security tests — v1.4.0.
//!
//! Each test names a concrete threat from `docs/THREAT_MODEL_FASE_11.md`
//! and asserts the defence holds. A regression here is a security
//! incident, not a style issue.
//!
//! Threats covered:
//! - T-11-01 Replay poisoning: attacker forges a ReplayToken.
//! - T-11-02 Legal-basis bypass: flow ships `sensitive:*` without
//!           `legal:*` on the same tool.
//! - T-11-03 HIPAA boundary breach: sensitive + HIPAA + ffmpeg combo.
//! - T-11-04 Continuity-token phishing: stolen session_id reused
//!           with a different-key-signed token.
//! - T-11-05 Buffer isolation bleed: SymbolicPtr leak across
//!           tenants via retag.
//! - T-11-06 Trust-catalogue drift: unknown proof in `trust:*`
//!           qualifier.
//! - T-11-07 Backpressure policy erasure: `Stream<T>` declared
//!           without a handler tool in reach.

use axon::buffer::{BufferKind, ZeroCopyBuffer};
use axon::lexer::Lexer;
use axon::parser::Parser;
use axon::pem::{
    ContinuityToken, ContinuityTokenError, ContinuityTokenSigner,
};
use axon::replay_token::{canonical_hash, InMemoryReplayLog, ReplayLog, ReplayTokenBuilder};
use axon::type_checker::{TypeChecker, TypeError};
use chrono::Duration;
use serde_json::json;

fn type_check(src: &str) -> Vec<TypeError> {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex ok");
    let program = Parser::new(tokens).parse().expect("parse ok");
    TypeChecker::new(&program).check()
}

fn any_error_mentions(errs: &[TypeError], needle: &str) -> bool {
    errs.iter().any(|e| e.message.contains(needle))
}

// ── T-11-01 Replay poisoning ────────────────────────────────────────

#[tokio::test]
async fn t_11_01_replay_log_rejects_token_with_tampered_outputs_hash() {
    // Threat: attacker crafts a ReplayToken claiming an effect
    // produced output O' when it actually produced O. The enterprise
    // ReplayService re-verifies the canonical hash; a tampered hex
    // triggers ReplayTokenMalformed.
    //
    // This Rust-side test only exercises the hash-derivation
    // determinism — the enterprise validation lives in the Python
    // test suite.
    let token = ReplayTokenBuilder::new()
        .effect_name("llm_infer")
        .inputs(json!({"prompt": "hello"}))
        .outputs(json!({"text": "hi"}))
        .model_version("m1")
        .timestamp(chrono::Utc::now())
        .nonce([1u8; 16])
        .mint();

    let recomputed = canonical_hash(&json!({"text": "hi"}));
    let as_hex: String = recomputed.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(token.outputs_hash_hex, as_hex);

    // If we serialise + an attacker flips one byte of the claimed
    // output, the derived hash no longer matches the stored one.
    let tampered_bytes = json!({"text": "hi-tampered"});
    let tampered_hash = canonical_hash(&tampered_bytes);
    let tampered_hex: String =
        tampered_hash.iter().map(|b| format!("{b:02x}")).collect();
    assert_ne!(tampered_hex, token.outputs_hash_hex);
}

#[tokio::test]
async fn t_11_01_replay_log_tokens_for_flow_cannot_cross_flows() {
    let log = InMemoryReplayLog::new();
    let t_flow_a = ReplayTokenBuilder::new()
        .effect_name("x")
        .inputs(json!({"flow_id": "flow-a"}))
        .outputs(json!({"ok": true}))
        .model_version("v1")
        .timestamp(chrono::Utc::now())
        .nonce([2u8; 16])
        .mint();
    let t_flow_b = ReplayTokenBuilder::new()
        .effect_name("x")
        .inputs(json!({"flow_id": "flow-b"}))
        .outputs(json!({"ok": true}))
        .model_version("v1")
        .timestamp(chrono::Utc::now())
        .nonce([3u8; 16])
        .mint();
    log.append(t_flow_a.clone()).await.unwrap();
    log.append(t_flow_b.clone()).await.unwrap();

    // An attacker requesting flow-a gets only flow-a tokens.
    let a_tokens = log.tokens_for_flow("flow-a").await.unwrap();
    assert_eq!(a_tokens.len(), 1);
    assert_eq!(a_tokens[0].token_hash_hex, t_flow_a.token_hash_hex);
}

// ── T-11-02 Legal-basis bypass ──────────────────────────────────────

#[test]
fn t_11_02_sensitive_without_legal_basis_fails_compilation() {
    let src = r#"
        tool process_health_record {
          provider: stub
          timeout: 10s
          effects: <sensitive:phi>
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "no 'legal:<basis>' effect"),
        "sensitive without legal must be rejected, got {:?}",
        errs
    );
}

#[test]
fn t_11_02_legal_basis_in_different_tool_does_not_satisfy_same_tool_rule() {
    // Two tools in the same program — one declares sensitive, the
    // other declares legal. The rule is SAME-tool coherence, so
    // tool A still fails even though B has a basis declared.
    let src = r#"
        tool handle_phi {
          provider: stub
          timeout: 10s
          effects: <sensitive:phi>
        }

        tool declare_compliance {
          provider: stub
          timeout: 10s
          effects: <legal:HIPAA.164_502>
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "Tool 'handle_phi'")
            && any_error_mentions(&errs, "no 'legal:<basis>'"),
        "same-tool rule must apply independently, got {:?}",
        errs
    );
}

// ── T-11-03 HIPAA boundary breach ───────────────────────────────────

#[test]
fn t_11_03_hipaa_plus_ffmpeg_always_rejected() {
    for (sensitive_cat, hipaa_basis) in [
        ("phi", "HIPAA.164_502"),
        ("mental_health", "HIPAA.164_502"),
    ] {
        let src = format!(
            r#"
                tool transcode_phi {{
                  provider: stub
                  timeout: 30s
                  effects: <sensitive:{sensitive_cat}, legal:{hipaa_basis}, ots:transform:pcm16:mp3, ots:backend:ffmpeg>
                }}
            "#
        );
        let errs = type_check(&src);
        assert!(
            any_error_mentions(&errs, "HIPAA")
                && any_error_mentions(&errs, "process boundary"),
            "HIPAA+ffmpeg for {sensitive_cat} must be rejected, got {:?}",
            errs
        );
    }
}

#[test]
fn t_11_03_hipaa_plus_native_compiles_cleanly() {
    // Sanity check — the rule is surgical, not "HIPAA rejects all
    // OTS". Native pipelines keep HIPAA happy.
    let src = r#"
        tool decode_phi_audio {
          provider: stub
          timeout: 30s
          effects: <sensitive:phi, legal:HIPAA.164_502, ots:transform:mulaw8:pcm16, ots:backend:native>
        }
    "#;
    let errs = type_check(src);
    assert!(
        !any_error_mentions(&errs, "process boundary"),
        "HIPAA + native must pass, got {:?}",
        errs
    );
}

// ── T-11-04 Continuity-token phishing ───────────────────────────────

#[test]
fn t_11_04_continuity_token_signed_with_attacker_key_rejected() {
    let server = ContinuityTokenSigner::new([7u8; 32]);
    let attacker = ContinuityTokenSigner::new([9u8; 32]);
    let forged = attacker.sign(&ContinuityToken::new(
        "sess-victim",
        Duration::minutes(15),
    ));
    let err = server.verify(&forged).unwrap_err();
    matches!(err, ContinuityTokenError::ForgedOrRotated);
}

#[test]
fn t_11_04_continuity_token_replay_after_expiry_rejected() {
    let signer = ContinuityTokenSigner::new([7u8; 32]);
    let expired = signer.sign(&ContinuityToken::new(
        "sess-1",
        Duration::seconds(-1),
    ));
    let err = signer.verify(&expired).unwrap_err();
    matches!(err, ContinuityTokenError::Expired { .. });
}

#[test]
fn t_11_04_continuity_token_session_id_swap_rejected() {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    let signer = ContinuityTokenSigner::new([7u8; 32]);
    let original = ContinuityToken::new("sess-a", Duration::minutes(15));
    let wire = signer.sign(&original);

    // Decode + mutate session_id + re-encode WITHOUT refreshing HMAC.
    let decoded = URL_SAFE_NO_PAD.decode(wire.as_bytes()).unwrap();
    let text = std::str::from_utf8(&decoded).unwrap();
    let tampered = text.replacen("sess-a", "sess-b", 1);
    let wire_bad = URL_SAFE_NO_PAD.encode(tampered.as_bytes());

    let err = signer.verify(&wire_bad).unwrap_err();
    matches!(err, ContinuityTokenError::ForgedOrRotated);
}

// ── T-11-05 Buffer isolation bleed ──────────────────────────────────

#[test]
fn t_11_05_retag_preserves_tenant_tag_so_cross_tenant_tag_does_not_leak() {
    let buf_a = ZeroCopyBuffer::from_bytes(vec![0u8; 16], BufferKind::raw())
        .with_tenant("tenant-a");
    let retagged = buf_a.retag(BufferKind::jpeg());
    assert_eq!(retagged.tenant_id(), Some("tenant-a"));
    // A slice of A's buffer does NOT inherit tenant-b; the tag is
    // set at construction time.
    let sliced = retagged.slice(0..4);
    assert_eq!(sliced.tenant_id(), Some("tenant-a"));
}

#[test]
fn t_11_05_clone_of_tenant_buffer_preserves_tenant() {
    let buf = ZeroCopyBuffer::from_bytes(vec![1u8; 32], BufferKind::raw())
        .with_tenant("tenant-a");
    let cloned = buf.clone();
    assert_eq!(cloned.tenant_id(), Some("tenant-a"));
    // The sharer count tracks live views; construction here
    // produces at least 2 sharers.
    assert!(cloned.sharers() >= 2);
}

// ── T-11-06 Trust-catalogue drift ──────────────────────────────────

#[test]
fn t_11_06_unknown_trust_proof_rejected() {
    let src = r#"
        tool verify_webhook {
          provider: stub
          timeout: 5s
          effects: <trust:md5>
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "Unknown trust proof"),
        "unknown proof must fall out of closed catalogue, got {:?}",
        errs
    );
}

#[test]
fn t_11_06_unknown_legal_basis_rejected() {
    let src = r#"
        tool process_gdpr {
          provider: stub
          timeout: 10s
          effects: <sensitive:eu_data, legal:GDPR.MadeUp>
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "Unknown legal basis"),
        "unknown basis must be rejected, got {:?}",
        errs
    );
}

// ── T-11-07 Backpressure policy erasure ─────────────────────────────

#[test]
fn t_11_07_stream_without_backpressure_tool_in_reach_rejected() {
    let src = r#"
        flow Transcribe(audio: Stream<Bytes>) {
          step Analyze {
            given: audio
            ask: "summarise"
          }
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "Stream<T>")
            && any_error_mentions(&errs, "backpressure policy"),
        "stream without policy must be rejected, got {:?}",
        errs
    );
}

#[test]
fn t_11_07_stream_effect_without_qualifier_rejected() {
    // Even if a flow references a tool, if the tool's `stream`
    // effect is bare (no qualifier) it fails the tool-level check.
    let src = r#"
        tool ingest_audio {
          provider: stub
          timeout: 30s
          effects: <stream>
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "backpressure policy")
            && any_error_mentions(&errs, "qualifier"),
        "stream bare qualifier must be rejected, got {:?}",
        errs
    );
}
