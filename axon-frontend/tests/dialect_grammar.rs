//! v1.28.0 — Parser grammar tests for the
//! parametrized SSE wire-format dialect grammar
//! `transport: sse(<dialect>)`.
//!
//! Pins:
//!   1. Bare `transport: sse` parses cleanly with `transport_dialect == ""`
//!      (D6 backwards-compat — pre-33.z.k.b grammar still works).
//!   2. `transport: sse(axon)` / `sse(openai)` / `sse(anthropic)` all
//!      parse cleanly with the dialect captured in `transport_dialect`.
//!   3. `transport: sse(<unknown>)` errors with a smart-suggest hint.
//!   4. `transport: json(openai)` errors — dialect param only valid on sse.
//!   5. `transport: ndjson(openai)` errors — same.
//!   6. Missing `)` errors with diagnostic message.

use axon_frontend::ast::Declaration;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

fn parse(src: &str) -> Result<axon_frontend::ast::Program, axon_frontend::parser::ParseError> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse()
}

fn endpoint<'a>(prog: &'a axon_frontend::ast::Program, name: &str) -> &'a axon_frontend::ast::AxonEndpointDefinition {
    for decl in &prog.declarations {
        if let Declaration::AxonEndpoint(ae) = decl {
            if ae.name == name {
                return ae;
            }
        }
    }
    panic!("axonendpoint {name} not found");
}

// ─── section 1 — Bare `transport: sse` parses with empty dialect ──────────

#[test]
fn s1_bare_transport_sse_parses_with_empty_dialect_d6() {
    let src = "flow F() -> Unit { step S { ask: \"x\" output: Stream<Token> } }\n\
        axonendpoint E { method: POST path: \"/x\" execute: F transport: sse }";
    let prog = parse(src).expect("parse");
    let ae = endpoint(&prog, "E");
    assert_eq!(ae.transport, "sse");
    assert_eq!(ae.transport_explicit, true);
    assert_eq!(
        ae.transport_dialect, "",
        "33.z.k.b: bare `transport: sse` resolves dialect at runtime per the \
         algebraic-effect predicate; parser leaves transport_dialect empty"
    );
}

// ─── section 2 — `transport: sse(<dialect>)` parses dialect into AST ──────

#[test]
fn s2_transport_sse_axon_parses() {
    let src = "flow F() -> Unit { step S { ask: \"x\" output: Stream<Token> } }\n\
        axonendpoint E { method: POST path: \"/x\" execute: F transport: sse(axon) }";
    let prog = parse(src).expect("parse");
    let ae = endpoint(&prog, "E");
    assert_eq!(ae.transport, "sse");
    assert_eq!(ae.transport_dialect, "axon");
}

#[test]
fn s2_transport_sse_openai_parses() {
    let src = "flow F() -> Unit { step S { ask: \"x\" output: Stream<Token> } }\n\
        axonendpoint E { method: POST path: \"/x\" execute: F transport: sse(openai) }";
    let prog = parse(src).expect("parse");
    let ae = endpoint(&prog, "E");
    assert_eq!(ae.transport, "sse");
    assert_eq!(ae.transport_dialect, "openai");
}

#[test]
fn s2_transport_sse_anthropic_parses() {
    let src = "flow F() -> Unit { step S { ask: \"x\" output: Stream<Token> } }\n\
        axonendpoint E { method: POST path: \"/x\" execute: F transport: sse(anthropic) }";
    let prog = parse(src).expect("parse");
    let ae = endpoint(&prog, "E");
    assert_eq!(ae.transport_dialect, "anthropic");
}

// ─── section 3 — Unknown dialect errors with smart-suggest ────────────────

#[test]
fn s3_unknown_dialect_errors_with_smart_suggest() {
    let src = "flow F() -> Unit { step S { ask: \"x\" output: Stream<Token> } }\n\
        axonendpoint E { method: POST path: \"/x\" execute: F transport: sse(opennai) }";
    let err = parse(src).expect_err("should reject unknown dialect");
    let msg = err.message.to_lowercase();
    assert!(
        msg.contains("invalid sse dialect") && msg.contains("opennai"),
        "33.z.k.b: unknown dialect must error explicitly. Got: {}",
        err.message
    );
    assert!(
        msg.contains("openai") || msg.contains("expected"),
        "33.z.k.b: smart-suggest should mention `openai` as nearest match. Got: {}",
        err.message
    );
}

// ─── section 4 — `json(<dialect>)` errors — only sse takes parametrization ─

#[test]
fn s4_json_dialect_param_errors() {
    let src = "flow F() -> Unit { step S { ask: \"x\" } }\n\
        axonendpoint E { method: POST path: \"/x\" execute: F transport: json(openai) }";
    let err = parse(src).expect_err("should reject json(openai)");
    let msg = err.message.to_lowercase();
    assert!(
        msg.contains("only valid for `sse`") || msg.contains("json"),
        "33.z.k.b: dialect param on non-sse base must error. Got: {}",
        err.message
    );
}

// ─── section 5 — `ndjson(<dialect>)` errors — same rule ───────────────────

#[test]
fn s5_ndjson_dialect_param_errors() {
    let src = "flow F() -> Unit { step S { ask: \"x\" output: Stream<Token> } }\n\
        axonendpoint E { method: POST path: \"/x\" execute: F transport: ndjson(axon) }";
    let err = parse(src).expect_err("should reject ndjson(axon)");
    assert!(
        err.message.to_lowercase().contains("only valid for `sse`") ||
        err.message.to_lowercase().contains("ndjson"),
        "33.z.k.b: dialect param on ndjson must error. Got: {}",
        err.message
    );
}

// ─── section 6 — Missing closing paren errors ─────────────────────────────

#[test]
fn s6_missing_rparen_errors() {
    let src = "flow F() -> Unit { step S { ask: \"x\" output: Stream<Token> } }\n\
        axonendpoint E { method: POST path: \"/x\" execute: F transport: sse(openai }";
    let err = parse(src).expect_err("should reject missing rparen");
    let msg = err.message.to_lowercase();
    assert!(
        msg.contains("expected `)`") || msg.contains("rparen") || msg.contains(")") || msg.contains("paren"),
        "33.z.k.b: missing closing paren must error. Got: {}",
        err.message
    );
}

// ─── section 7 — Closed-catalog cardinality pin (defensive) ───────────────

#[test]
fn s7_dialect_catalog_exact_five_q3_revised() {
    use axon_frontend::parser::AXONENDPOINT_TRANSPORT_DIALECTS;
    assert_eq!(
        AXONENDPOINT_TRANSPORT_DIALECTS.len(),
        5,
        "33.z.k.b Q3 revised 2026-05-14: vertical-grounded scope = \
         exactly 5 dialects {{axon, openai, kimi, glm, anthropic}}. \
         kimi + glm added as first-class entries (adopter pipelines \
         through Moonshot Kimi + Zhipu ChatGLM); their wire is \
         byte-identical to openai (OpenAI-compat Chat Completions). \
         Adding a 6th requires a deliberate step + cross-stack \
         drift gate update."
    );
    let expected: std::collections::HashSet<&str> =
        ["axon", "openai", "kimi", "glm", "anthropic"]
            .iter()
            .copied()
            .collect();
    let actual: std::collections::HashSet<&str> =
        AXONENDPOINT_TRANSPORT_DIALECTS.iter().copied().collect();
    assert_eq!(actual, expected, "33.z.k.b: closed-catalog set drift");
}

#[test]
fn s8_transport_sse_kimi_parses() {
    let src = "flow F() -> Unit { step S { ask: \"x\" output: Stream<Token> } }\n\
        axonendpoint E { method: POST path: \"/x\" execute: F transport: sse(kimi) }";
    let prog = parse(src).expect("parse");
    let ae = endpoint(&prog, "E");
    assert_eq!(ae.transport, "sse");
    assert_eq!(ae.transport_dialect, "kimi");
}

#[test]
fn s8_transport_sse_glm_parses() {
    let src = "flow F() -> Unit { step S { ask: \"x\" output: Stream<Token> } }\n\
        axonendpoint E { method: POST path: \"/x\" execute: F transport: sse(glm) }";
    let prog = parse(src).expect("parse");
    let ae = endpoint(&prog, "E");
    assert_eq!(ae.transport, "sse");
    assert_eq!(ae.transport_dialect, "glm");
}
