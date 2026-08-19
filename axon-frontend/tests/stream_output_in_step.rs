//! v1.23.0 — Rust frontend parser captures `output: Stream<T>` inside
//! step bodies (closes the disjunct-(a) AST gap).
//!
//! ## Adopter case (2026-05-12)
//!
//! Surfaced by adopter migration audit `docs/MIGRATION_TO_AXON.md` after
//! v1.23.0 shipped: dynamic routes serving the adopter's stream-effect
//! flow returned `application/json` instead of `text/event-stream` even
//! after the v1.23.0 path registration + v1.23.0 per-route classifier
//! arrived. Adopter trail: 9 version bumps 1.16.2 → 1.23.0; canonical
//! syntax per `docs/STREAM_EFFECTS.md`; parse + compile OK; runtime
//! emitted JSON wrapper.
//!
//! ## Root cause
//!
//! `Parser::parse_step` for the `output:` field consumed only a bare
//! `Identifier` token: `node.output_type = consume(Identifier)?.value`.
//! For `output: Stream<Token>`, this captured `"Stream"` and left
//! `<Token>` unconsumed. Then `type_checker::flow_has_stream_output`
//! checked `starts_with("Stream<") && ends_with('>')` — false →
//! `produces_stream` false → `implicit_transport == "json"` → dynamic
//! route fallback served JSON.
//!
//! Python parser was fixed for this same gap 2026-05-09 via
//! `_parse_output_type_string`. The Rust frontend lagged — fixed here
//! by introducing the mirror `parse_output_type_string` + threading it
//! into `parse_step`'s `Output` case.
//!
//! ## Pillar trace per D12 (v1.23.0)
//!
//! - **MATHEMATICS** — the inference function `produces_stream` is
//!   now total over `output: Stream<T>` inputs (was undefined / wrong
//!   pre-fix because the AST projection dropped the `<T>` payload).
//! - **LOGIC** — declarative source IS the wire format; the parser
//!   now captures the FULL declared shape so the inference predicate
//!   sees what the adopter wrote.
//! - **PHILOSOPHY** — adopters never diagnose our bugs we diagnose
//!   theirs; this is exactly what the Kivi/MIGRATION_TO_AXON adopter
//!   trail surfaced.
//! - **COMPUTING** — the fix is cross-stack symmetric with Python;
//!   D11 drift-gate parity restored.

use axon_frontend::ast::{Declaration, FlowStep, Program};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::{
    compute_implicit_transports, implicit_transport, produces_stream,
};

fn parse(source: &str) -> Program {
    let tokens = Lexer::new(source, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

// ─── section 1 — parser captures the full Stream<T> shape ──────────────────

#[test]
fn step_output_stream_of_t_captured_as_stream_lt_t_gt() {
    let src = "flow F() -> Unit {\n\
               step Generate { ask: \"x\" output: Stream<Token> }\n\
               }\n\
               axonendpoint E { method: POST path: \"/f\" execute: F }";
    let program = parse(src);
    let flow = program
        .declarations
        .iter()
        .find_map(|d| if let Declaration::Flow(f) = d { Some(f) } else { None })
        .expect("flow F");
    let step = flow
        .body
        .iter()
        .find_map(|s| if let FlowStep::Step(st) = s { Some(st) } else { None })
        .expect("step Generate");
    assert_eq!(
        step.output_type, "Stream<Token>",
        "pre-fix this was just \"Stream\" — the disjunct (a) AST gap"
    );
}

#[test]
fn step_output_bare_identifier_unaffected() {
    // Regression guard — non-generic `output: Int` keeps prior behavior.
    let src = "flow F() -> Int {\n\
               step S { ask: \"x\" output: Int }\n\
               }\n\
               axonendpoint E { method: POST path: \"/f\" execute: F }";
    let program = parse(src);
    let flow = program
        .declarations
        .iter()
        .find_map(|d| if let Declaration::Flow(f) = d { Some(f) } else { None })
        .expect("flow F");
    let step = flow
        .body
        .iter()
        .find_map(|s| if let FlowStep::Step(st) = s { Some(st) } else { None })
        .expect("step S");
    assert_eq!(step.output_type, "Int");
}

#[test]
fn step_output_optional_identifier_round_trip() {
    let src = "flow F() -> Unit {\n\
               step S { ask: \"x\" output: Foo? }\n\
               }\n\
               axonendpoint E { method: POST path: \"/f\" execute: F }";
    let program = parse(src);
    let flow = program
        .declarations
        .iter()
        .find_map(|d| if let Declaration::Flow(f) = d { Some(f) } else { None })
        .expect("flow F");
    let step = flow
        .body
        .iter()
        .find_map(|s| if let FlowStep::Step(st) = s { Some(st) } else { None })
        .expect("step S");
    assert_eq!(step.output_type, "Foo?");
}

#[test]
fn step_output_optional_stream_round_trip() {
    let src = "flow F() -> Unit {\n\
               step S { ask: \"x\" output: Stream<Token>? }\n\
               }\n\
               axonendpoint E { method: POST path: \"/f\" execute: F }";
    let program = parse(src);
    let flow = program
        .declarations
        .iter()
        .find_map(|d| if let Declaration::Flow(f) = d { Some(f) } else { None })
        .expect("flow F");
    let step = flow
        .body
        .iter()
        .find_map(|s| if let FlowStep::Step(st) = s { Some(st) } else { None })
        .expect("step S");
    assert_eq!(step.output_type, "Stream<Token>?");
}

// ─── section 2 — produces_stream now fires on disjunct (a) ────────────────

#[test]
fn produces_stream_fires_on_step_output_stream_t_disjunct_a() {
    let src = "flow F() -> Unit {\n\
               step Generate { ask: \"x\" output: Stream<Token> }\n\
               }\n\
               axonendpoint E { method: POST path: \"/f\" execute: F }";
    let program = parse(src);
    let flow = program
        .declarations
        .iter()
        .find_map(|d| if let Declaration::Flow(f) = d { Some(f) } else { None })
        .expect("flow F");
    assert!(
        produces_stream(flow, &program),
        "disjunct (a): step with `output: Stream<T>` MUST mark the flow as stream-producing"
    );
}

#[test]
fn produces_stream_does_not_fire_on_non_stream_step() {
    let src = "flow F() -> Int {\n\
               step S { ask: \"x\" output: Int }\n\
               }\n\
               axonendpoint E { method: POST path: \"/f\" execute: F }";
    let program = parse(src);
    let flow = program
        .declarations
        .iter()
        .find_map(|d| if let Declaration::Flow(f) = d { Some(f) } else { None })
        .expect("flow F");
    assert!(!produces_stream(flow, &program));
}

// ─── section 3 — implicit_transport resolves to "sse" via disjunct (a) ────

#[test]
fn implicit_transport_resolves_to_sse_when_step_has_stream_output() {
    let src = "flow F() -> Unit {\n\
               step Generate { ask: \"x\" output: Stream<Token> }\n\
               }\n\
               axonendpoint E { method: POST path: \"/f\" execute: F }";
    let program = parse(src);
    let flow = program
        .declarations
        .iter()
        .find_map(|d| if let Declaration::Flow(f) = d { Some(f) } else { None })
        .expect("flow F");
    let endpoint = program
        .declarations
        .iter()
        .find_map(|d| if let Declaration::AxonEndpoint(e) = d { Some(e) } else { None })
        .expect("axonendpoint E");
    let inferred = implicit_transport(endpoint, Some(flow), &program);
    assert_eq!(
        inferred, "sse",
        "the wire-layer adopter trail: source declares stream effect via \
         disjunct (a), inference MUST return sse (D1 fires); pre-fix this \
         returned json"
    );
}

#[test]
fn compute_implicit_transports_populates_ast_field_for_disjunct_a() {
    let src = "flow F() -> Unit {\n\
               step Generate { ask: \"x\" output: Stream<Token> }\n\
               }\n\
               axonendpoint E { method: POST path: \"/f\" execute: F }";
    let mut program = parse(src);
    compute_implicit_transports(&mut program);
    let endpoint = program
        .declarations
        .iter()
        .find_map(|d| if let Declaration::AxonEndpoint(e) = d { Some(e) } else { None })
        .expect("axonendpoint E");
    assert_eq!(
        endpoint.implicit_transport, "sse",
        "compute_implicit_transports populates the AxonEndpoint.implicit_transport \
         field at deploy-time; the dynamic-route fallback in axon-rs reads THIS \
         field via DynamicEndpointRoute. Pre-fix it was \"json\"."
    );
}

// ─── section 4 — explicit transport: json overrides disjunct (a) (D3) ─────

#[test]
fn explicit_transport_json_overrides_disjunct_a_inference_d3() {
    let src = "flow F() -> Unit {\n\
               step Generate { ask: \"x\" output: Stream<Token> }\n\
               }\n\
               axonendpoint E { method: POST path: \"/f\" execute: F transport: json }";
    let program = parse(src);
    let flow = program
        .declarations
        .iter()
        .find_map(|d| if let Declaration::Flow(f) = d { Some(f) } else { None })
        .expect("flow F");
    let endpoint = program
        .declarations
        .iter()
        .find_map(|d| if let Declaration::AxonEndpoint(e) = d { Some(e) } else { None })
        .expect("axonendpoint E");
    // D3 sacred opt-out: even with disjunct (a) firing inside the flow,
    // explicit `transport: json` always wins.
    let inferred = implicit_transport(endpoint, Some(flow), &program);
    assert_eq!(inferred, "json");
}

// ─── section 5 — cross-stack contract anchor (D11) ─────────────────────────

#[test]
fn cross_stack_anchor_python_already_handles_disjunct_a() {
    // This anchor documents that the SHIPPING Python parser already
    // handled disjunct (a) (fixed 2026-05-09 via
    // `_parse_output_type_string`). The Rust frontend lagged until
    // this fix. With both stacks now in lockstep, the D11 drift-gate
    // contract from v1.21.0 + v1.22.0 is restored for the Stream<T>
    // step-output shape. No corpus addition is required: the existing
    // v1.21.0 drift-gate corpus has entries flagged
    // `rust_parser_known_gap: true` for this exact disjunct; once this
    // fix is committed, those flags can be flipped back to default —
    // tracked separately as a follow-on cleanup.
    //
    // The test passes trivially; its presence documents the contract.
}
