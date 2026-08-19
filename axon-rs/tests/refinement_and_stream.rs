//! Integration tests for v1.4.0 — Temporal Algebraic
//! Effects + Trust Types.
//!
//! These exercise the compiler end-to-end: lex → parse → type-check,
//! asserting the diagnostics the new pass emits for each adversarial
//! scenario.
//!
//! Axon syntax reminders (see `examples/*.axon`):
//! - `tool Name { provider: X, timeout: 10s, effects: <kind:qual, ...> }`
//! - `flow Name(param: Type) { step X { given: y ask: "..." apply: tool } }`

use axon::lexer::Lexer;
use axon::parser::Parser;
use axon::type_checker::{TypeChecker, TypeError};

fn try_type_check(src: &str) -> Option<Vec<TypeError>> {
    let tokens = Lexer::new(src, "t.axon").tokenize().ok()?;
    let program = Parser::new(tokens).parse().ok()?;
    Some(TypeChecker::new(&program).check())
}

fn type_check(src: &str) -> Vec<TypeError> {
    try_type_check(src).expect("expected source to lex + parse")
}

fn any_error_mentions(errs: &[TypeError], needle: &str) -> bool {
    errs.iter().any(|e| e.message.contains(needle))
}

// ── Tool-level effect qualifier checks ──────────────────────────────

#[test]
fn stream_effect_without_qualifier_is_rejected() {
    // The `stream` base effect without a backpressure qualifier is
    // an error; the catalogue name alone doesn't imply a policy.
    let src = r#"
        tool record_audio {
          provider: stub
          timeout: 30s
          effects: <stream>
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "stream") && any_error_mentions(&errs, "backpressure"),
        "expected qualifier-missing error, got: {:?}",
        errs
    );
}

#[test]
fn stream_effect_with_unknown_qualifier_is_rejected() {
    let src = r#"
        tool record_audio {
          provider: stub
          timeout: 30s
          effects: <stream:retry_forever>
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "Unknown backpressure policy"),
        "expected unknown-policy error, got: {:?}",
        errs
    );
}

#[test]
fn stream_effect_with_valid_qualifier_passes() {
    let src = r#"
        tool record_audio {
          provider: stub
          timeout: 30s
          effects: <stream:drop_oldest>
        }
    "#;
    let errs = type_check(src);
    assert!(
        !any_error_mentions(&errs, "backpressure")
            && !any_error_mentions(&errs, "Unknown"),
        "unexpected errors: {:?}",
        errs
    );
}

#[test]
fn trust_effect_without_qualifier_is_rejected() {
    let src = r#"
        tool verify_webhook {
          provider: stub
          timeout: 5s
          effects: <trust>
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "trust") && any_error_mentions(&errs, "proof"),
        "expected proof-missing error, got: {:?}",
        errs
    );
}

#[test]
fn trust_effect_with_unknown_qualifier_is_rejected() {
    let src = r#"
        tool verify_webhook {
          provider: stub
          timeout: 5s
          effects: <trust:crc32>
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "Unknown trust proof"),
        "expected unknown-proof error, got: {:?}",
        errs
    );
}

#[test]
fn trust_effect_with_all_catalog_proofs_passes() {
    // Each proof from the closed catalogue is accepted.
    for proof in ["hmac", "jwt_sig", "oauth_code_exchange", "ed25519"] {
        let src = format!(
            r#"
                tool verify_payload {{
                  provider: stub
                  timeout: 5s
                  effects: <trust:{proof}>
                }}
            "#
        );
        let errs = type_check(&src);
        assert!(
            !any_error_mentions(&errs, "Unknown trust proof"),
            "expected {proof} to pass, got: {:?}",
            errs
        );
    }
}

// ── Flow-level refinement + stream contracts ────────────────────────

#[test]
fn flow_with_stream_parameter_requires_backpressure_tool() {
    // Flow receives a Stream<Bytes> but no tool in its reach declares
    // a `stream:<policy>` effect — compile error.
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
        "expected Stream-without-backpressure error, got: {:?}",
        errs
    );
}

#[test]
fn flow_with_stream_and_matching_tool_passes() {
    let src = r#"
        tool ingest_audio {
          provider: stub
          timeout: 30s
          effects: <stream:drop_oldest>
        }

        flow Transcribe(audio: Stream<Bytes>) {
          step Analyze {
            given: audio
            ask: "summarise"
            apply: ingest_audio
          }
        }
    "#;
    let errs = type_check(src);
    assert!(
        !any_error_mentions(&errs, "backpressure policy"),
        "did not expect v1.4.0 errors, got: {:?}",
        errs
    );
}

#[test]
fn flow_with_untrusted_parameter_requires_verifier_tool() {
    let src = r#"
        flow HandleWebhook(body: Untrusted<HttpBody>) {
          step Process {
            given: body
            ask: "parse"
          }
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "Untrusted<T>")
            && any_error_mentions(&errs, "catalogue verifiers"),
        "expected Untrusted-without-verifier error, got: {:?}",
        errs
    );
}

#[test]
fn flow_with_untrusted_and_matching_verifier_tool_passes() {
    let src = r#"
        tool verify_signature {
          provider: stub
          timeout: 5s
          effects: <trust:hmac>
        }

        flow HandleWebhook(body: Untrusted<HttpBody>) {
          step Verify {
            given: body
            ask: "authenticate"
            apply: verify_signature
          }
        }
    "#;
    let errs = type_check(src);
    assert!(
        !any_error_mentions(&errs, "catalogue verifiers"),
        "did not expect v1.4.0 errors, got: {:?}",
        errs
    );
}

#[test]
fn flow_returning_stream_also_requires_backpressure() {
    let src = r#"
        flow EmitMetrics() -> Stream<Metric> {
          step Produce {
            given: nothing
            ask: "emit"
          }
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "Stream<T>")
            && any_error_mentions(&errs, "backpressure policy"),
        "expected Stream-return-without-backpressure error, got: {:?}",
        errs
    );
}

// ── Catalogue coverage regression ───────────────────────────────────

#[test]
fn backpressure_catalog_is_closed_to_typos() {
    // A one-letter typo must be caught. Regressions here mean the
    // `is_valid` check accepted an unknown qualifier.
    let src = r#"
        tool x {
          provider: stub
          timeout: 5s
          effects: <stream:pauseupstream>
        }
    "#;
    let errs = type_check(src);
    assert!(
        any_error_mentions(&errs, "Unknown backpressure policy"),
        "typo'd qualifier must be rejected, got: {:?}",
        errs
    );
}

#[test]
fn trust_catalog_is_closed_to_case_variants() {
    let src = r#"
        tool x {
          provider: stub
          timeout: 5s
          effects: <trust:HMAC>
        }
    "#;
    let errs = type_check(src);
    // Case-sensitive — upper-case HMAC is not in the catalogue.
    assert!(
        any_error_mentions(&errs, "Unknown trust proof"),
        "case-variant must be rejected, got: {:?}",
        errs
    );
}
