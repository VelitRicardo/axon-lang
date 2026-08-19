//! v1.22.0 — Rust mirror tests for the `axon-W001` warning emission.
//!
//! D4 + D7 cross-stack contract — the Rust mirror produces
//! byte-identical warning text for byte-identical input.
//!
//! Pillar trace per D10:
//!   PHILOSOPHY — the language must be honest about its inferences.
//!   LOGIC      — the warning fires iff a precise predicate holds.
//!   COMPUTING  — rate-limited per-endpoint; suppression explicit.

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::{
    compute_implicit_transport_warnings, compute_implicit_transports, W001_CODE,
};

fn check(src: &str) -> Vec<axon_frontend::type_checker::TypeError> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let mut program = Parser::new(tokens).parse().expect("parse");
    compute_implicit_transports(&mut program);
    compute_implicit_transport_warnings(&program)
}

fn w001_count(warnings: &[axon_frontend::type_checker::TypeError]) -> usize {
    warnings
        .iter()
        .filter(|w| w.message.contains(W001_CODE))
        .count()
}

// ─── 1. Positive — warning fires on every implicit-sse site ──────────

#[test]
fn kivi_shape_fires() {
    let src = "tool chat_token_stream { description: \"streaming\" effects: <stream:drop_oldest> }\n\
               flow Chat() -> String { step Generate { ask: \"hi\" apply: chat_token_stream } }\n\
               axonendpoint ChatEndpoint { method: POST path: \"/chat\" execute: Chat }";
    let warnings = check(src);
    assert_eq!(w001_count(&warnings), 1);
    let msg = &warnings[0].message;
    assert!(msg.contains("ChatEndpoint"), "msg: {msg}");
    assert!(msg.contains("Chat"));
    assert!(msg.contains("chat_token_stream"));
    assert!(msg.contains("stream:drop_oldest"));
}

#[test]
fn all_four_backpressure_policies_fire() {
    for policy in ["drop_oldest", "degrade_quality", "pause_upstream", "fail"] {
        let src = format!(
            "tool t {{ description: \"t\" effects: <stream:{policy}> }}\n\
             flow F() -> Unit {{ step S {{ ask: \"x\" apply: t }} }}\n\
             axonendpoint F {{ method: POST path: \"/f\" execute: F }}"
        );
        let warnings = check(&src);
        assert_eq!(
            w001_count(&warnings),
            1,
            "policy {policy} expected 1 W001"
        );
        assert!(
            warnings[0].message.contains(&format!("stream:{policy}")),
            "policy {policy} message: {}",
            warnings[0].message
        );
    }
}

// ─── 2. Suppression rules (D3 opt-out + explicit declarations) ───────

#[test]
fn explicit_sse_suppresses_warning() {
    let src = "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
               flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
               axonendpoint F { method: POST path: \"/f\" execute: F transport: sse }";
    let warnings = check(src);
    assert_eq!(w001_count(&warnings), 0);
}

#[test]
fn explicit_json_suppresses_warning() {
    let src = "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
               flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
               axonendpoint F { method: POST path: \"/f\" execute: F transport: json }";
    let warnings = check(src);
    assert_eq!(w001_count(&warnings), 0);
}

#[test]
fn explicit_ndjson_suppresses_warning() {
    let src = "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
               flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
               axonendpoint F { method: POST path: \"/f\" execute: F transport: ndjson }";
    let warnings = check(src);
    assert_eq!(w001_count(&warnings), 0);
}

#[test]
fn no_stream_effect_no_warning() {
    let src = "flow F() -> Int { step S { ask: \"x\" output: Int } }\n\
               axonendpoint F { method: POST path: \"/f\" execute: F }";
    let warnings = check(src);
    assert_eq!(w001_count(&warnings), 0);
}

#[test]
fn orphan_execute_flow_no_warning() {
    let src = "axonendpoint Orphan { method: POST path: \"/o\" execute: Ghost }";
    let warnings = check(src);
    assert_eq!(w001_count(&warnings), 0);
}

// ─── 3. Rate-limiting — one warning per endpoint ─────────────────────

#[test]
fn rate_limit_one_warning_per_endpoint() {
    let src = "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
               flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
               axonendpoint F { method: POST path: \"/f\" execute: F }";
    let warnings = check(src);
    assert_eq!(w001_count(&warnings), 1);
}

#[test]
fn multiple_endpoints_same_flow_each_fires_once() {
    let src = "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
               flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
               axonendpoint E1 { method: POST path: \"/e1\" execute: F }\n\
               axonendpoint E2 { method: POST path: \"/e2\" execute: F }";
    let warnings = check(src);
    assert_eq!(w001_count(&warnings), 2);
    let msgs: Vec<&str> = warnings.iter().map(|w| w.message.as_str()).collect();
    assert!(msgs.iter().any(|m| m.contains("E1")));
    assert!(msgs.iter().any(|m| m.contains("E2")));
}

// ─── 4. Message shape ─────────────────────────────────────────────────

#[test]
fn message_starts_with_canonical_prefix() {
    let src = "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
               flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
               axonendpoint F { method: POST path: \"/f\" execute: F }";
    let warnings = check(src);
    let prefix = format!("warning[{W001_CODE}]:");
    assert!(
        warnings[0].message.starts_with(&prefix),
        "message: {}",
        warnings[0].message
    );
}

#[test]
fn message_mentions_both_remediation_paths() {
    let src = "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
               flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
               axonendpoint F { method: POST path: \"/f\" execute: F }";
    let warnings = check(src);
    let msg = &warnings[0].message;
    assert!(msg.contains("transport: sse"));
    assert!(msg.contains("transport: json"));
}

#[test]
fn message_mentions_strict_flag() {
    let src = "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
               flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
               axonendpoint F { method: POST path: \"/f\" execute: F }";
    let warnings = check(src);
    assert!(warnings[0].message.contains("strict_type_driven_transport"));
}

#[test]
fn line_column_point_at_axonendpoint() {
    let src = "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
               flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
               axonendpoint Live { method: POST path: \"/f\" execute: F }";
    let warnings = check(src);
    assert_eq!(warnings[0].line, 3);
    assert!(warnings[0].column > 0);
}

// ─── 5. Idempotence ───────────────────────────────────────────────────

#[test]
fn idempotent_recomputation() {
    let src = "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
               flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
               axonendpoint F { method: POST path: \"/f\" execute: F }";
    let tokens = Lexer::new(src, "<idem>").tokenize().unwrap();
    let mut program = Parser::new(tokens).parse().unwrap();
    compute_implicit_transports(&mut program);
    let w1 = compute_implicit_transport_warnings(&program);
    let w2 = compute_implicit_transport_warnings(&program);
    assert_eq!(w1.len(), 1);
    assert_eq!(w1.len(), w2.len());
    assert_eq!(w1[0].message, w2[0].message);
}

// ─── 6. Origin description ────────────────────────────────────────────

#[test]
fn apply_ref_describes_step_and_tool() {
    let src = "tool brew { description: \"brew\" effects: <stream:fail> }\n\
               flow F() -> Unit { step Pour { ask: \"x\" apply: brew } }\n\
               axonendpoint F { method: POST path: \"/f\" execute: F }";
    let warnings = check(src);
    let msg = &warnings[0].message;
    assert!(msg.contains("step 'Pour'"), "msg: {msg}");
    assert!(msg.contains("tool 'brew'"), "msg: {msg}");
    assert!(msg.contains("stream:fail"), "msg: {msg}");
}

// ─── 7. Cross-stack message-shape parity (D7) ────────────────────────

#[test]
fn message_shape_matches_python_canonical_substrings() {
    // Cross-stack contract: the Rust message must contain the same
    // canonical phrases the Python reference implementation emits.
    // The Python pack tests for these substrings too; if both sides
    // pass, the drift gate holds for the message surface.
    let src = "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
               flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
               axonendpoint F { method: POST path: \"/f\" execute: F }";
    let warnings = check(src);
    let msg = &warnings[0].message;
    // Anchor phrases (verbatim across both stacks).
    for phrase in [
        "implicit `transport: sse` inferred from stream effects",
        "Declare `transport: sse` to silence this warning",
        "or `transport: json` to opt out",
        "`strict_type_driven_transport: true`",
        "/v1/execute",
    ] {
        assert!(
            msg.contains(phrase),
            "Rust W001 must contain anchor phrase {phrase:?} for cross-stack parity. msg:\n{msg}"
        );
    }
}
