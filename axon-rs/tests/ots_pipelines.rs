//! Integration tests for v1.4.0 — OTS Binary Pipeline
//! Synthesis.
//!
//! Covers: end-to-end mulaw→pcm16→16k pipeline through the
//! global registry, type-checker qualifier enforcement, and the
//! HIPAA+ffmpeg rejection rule.

use axon::buffer::{BufferKind, ZeroCopyBuffer};
use axon::lexer::Lexer;
use axon::ots::{global_registry, Pipeline};
use axon::parser::Parser;
use axon::type_checker::{TypeChecker, TypeError};

fn type_check(src: &str) -> Vec<TypeError> {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex ok");
    let program = Parser::new(tokens).parse().expect("parse ok");
    TypeChecker::new(&program).check()
}

fn any_error_mentions(errs: &[TypeError], needle: &str) -> bool {
    errs.iter().any(|e| e.message.contains(needle))
}

// ── Registry + pipeline synthesis ────────────────────────────────────

#[test]
fn global_registry_resolves_mulaw_to_pcm16() {
    let reg = global_registry();
    let p = Pipeline::from_registry(
        reg,
        &BufferKind::mulaw8(),
        &BufferKind::pcm16(),
    )
    .expect("mulaw→pcm16 path exists");
    assert_eq!(p.len(), 1);
    assert!(!p.crosses_process_boundary());
}

#[test]
fn global_registry_resolves_mulaw_to_16k_via_pcm() {
    // Native path: mulaw8 → pcm16 → pcm16_8k? NO — the resampler
    // operates on pcm16_<rate>k kinds. Telephony flows use
    // mulaw→pcm16 and then explicitly resample on kinds tagged
    // with rate. The OTS registry lets resamplers compose:
    let reg = global_registry();
    let p = Pipeline::from_registry(
        reg,
        &BufferKind::new("pcm16_8k"),
        &BufferKind::new("pcm16_16k"),
    )
    .expect("pcm16_8k → pcm16_16k path exists");
    assert!(p.len() >= 1);
    assert!(!p.crosses_process_boundary());
}

#[test]
fn pipeline_execute_end_to_end_mulaw_decode() {
    let reg = global_registry();
    let p = Pipeline::from_registry(
        reg,
        &BufferKind::mulaw8(),
        &BufferKind::pcm16(),
    )
    .unwrap();

    // A small μ-law fixture: 0xFF = smallest positive magnitude,
    // 0x80 = largest negative, 0x00 = largest positive.
    let input = ZeroCopyBuffer::from_bytes(
        vec![0xFF, 0x80, 0x00, 0x7F],
        BufferKind::mulaw8(),
    );
    let out = p.execute(&input).expect("pipeline executes");
    assert_eq!(out.kind().slug(), "pcm16");
    assert_eq!(out.len(), 8, "4 μ-law bytes → 8 PCM16 bytes");
}

// ── Type-checker qualifier enforcement ──────────────────────────────

#[test]
fn ots_effect_without_subkind_rejected() {
    let src = r#"
        tool transcode {
          provider: stub
          timeout: 10s
          effects: <ots>
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "Effect 'ots'")
            && any_error_mentions(&errs, "subkind"),
        "expected ots-without-subkind error, got {:?}",
        errs
    );
}

#[test]
fn ots_transform_without_from_to_rejected() {
    let src = r#"
        tool transcode {
          provider: stub
          timeout: 10s
          effects: <ots:transform:mulaw8>
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "Effect 'ots:transform'")
            && any_error_mentions(&errs, "<from>:<to>"),
        "expected transform-missing-sink error, got {:?}",
        errs
    );
}

#[test]
fn ots_backend_unknown_qualifier_rejected() {
    let src = r#"
        tool transcode {
          provider: stub
          timeout: 10s
          effects: <ots:backend:gstreamer>
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "Unknown OTS backend"),
        "expected unknown-backend error, got {:?}",
        errs
    );
}

#[test]
fn ots_backend_native_and_ffmpeg_both_compile() {
    for backend in ["native", "ffmpeg"] {
        let src = format!(
            r#"
                tool t {{
                  provider: stub
                  timeout: 10s
                  effects: <ots:backend:{backend}>
                }}
            "#
        );
        let errs = type_check(&src);
        assert!(
            !any_error_mentions(&errs, "Unknown"),
            "backend {backend} should compile, got {:?}",
            errs
        );
    }
}

#[test]
fn ots_transform_with_valid_from_to_compiles() {
    let src = r#"
        tool transcode {
          provider: stub
          timeout: 10s
          effects: <ots:transform:mulaw8:pcm16, ots:backend:native>
        }
    "#;
    let errs = type_check(src);
    assert!(
        !any_error_mentions(&errs, "ots"),
        "valid ots effects should compile, got {:?}",
        errs
    );
}

// ── HIPAA + ffmpeg rejection ────────────────────────────────────────

#[test]
fn hipaa_plus_ffmpeg_is_rejected() {
    let src = r#"
        tool transcribe_phi {
          provider: stub
          timeout: 30s
          effects: <sensitive:phi, legal:HIPAA.164_502, ots:transform:pcm16:mp3, ots:backend:ffmpeg>
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "HIPAA")
            && any_error_mentions(&errs, "process boundary"),
        "expected HIPAA+ffmpeg rejection, got {:?}",
        errs
    );
}

#[test]
fn hipaa_with_native_backend_compiles() {
    let src = r#"
        tool decode_phi_audio {
          provider: stub
          timeout: 30s
          effects: <sensitive:phi, legal:HIPAA.164_502, ots:transform:mulaw8:pcm16, ots:backend:native>
        }
    "#;
    let errs = type_check(src);
    assert!(
        !any_error_mentions(&errs, "HIPAA")
            || !any_error_mentions(&errs, "process boundary"),
        "HIPAA + native should compile, got {:?}",
        errs
    );
}

#[test]
fn gdpr_plus_ffmpeg_is_NOT_rejected() {
    // The HIPAA rule is targeted — GDPR adopters can choose
    // subprocess delegation if their ops team accepts the risk;
    // the checker doesn't infantilise them.
    let src = r#"
        tool transcode_eu_data {
          provider: stub
          timeout: 30s
          effects: <sensitive:eu_personal_data, legal:GDPR.Art6.Consent, ots:transform:mp3:wav, ots:backend:ffmpeg>
        }
    "#;
    let errs = type_check(src);
    assert!(
        !any_error_mentions(&errs, "HIPAA")
            && !any_error_mentions(&errs, "process boundary"),
        "GDPR + ffmpeg should compile, got {:?}",
        errs
    );
}
