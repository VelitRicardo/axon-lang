//! v2.4.0 — the `quant { … }` cognitive block surface
//! (`axon-enterprise/docs/cycle/quant_cognitive_primitive.md`;
//! paper `docs/papers/paper_primitiva_quant.md`).
//!
//! `quant` projects an MEK semantic tensor into a complex Hilbert space. This
//! test pins the FRONTEND surface only (51.a): the keyword parses as a flow-body
//! block, the optional `(key: value)` attribute header is order-free, the body
//! lowers to real nested `IRFlowNode`s (so v2.4.0 can scan it), the default
//! backend effect is `quant_sim`, unknown attributes are rejected, and a bare
//! `quant {}` serializes with the optional attributes serde-elided.
//!
//! NOT covered here (later steps): the Continuous Type Invariant over the
//! body (v2.4.0), the typed continuous grammar + `Observable` (v2.4.0), the
//! `quant_sim`/`qpu_native` effect injection + `yield` measurement (v2.4.0), and
//! the `QuantBackend` port + capped reference simulator (v2.4.0).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::ir_nodes::{IRFlowNode, IRProgram, IRQuant};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

fn ir_of(src: &str) -> IRProgram {
    let toks = Lexer::new(src, "t.axon").tokenize().expect("lex");
    let prog = Parser::new(toks).parse().expect("parse");
    IRGenerator::new().generate(&prog)
}

fn parse_result(src: &str) -> Result<(), String> {
    let toks = Lexer::new(src, "t.axon").tokenize().map_err(|e| format!("{e:?}"))?;
    Parser::new(toks).parse().map(|_| ()).map_err(|e| e.message)
}

fn quant_of(ir: &IRProgram, flow: &str) -> IRQuant {
    ir.flows
        .iter()
        .find(|f| f.name == flow)
        .expect("flow")
        .steps
        .iter()
        .find_map(|n| match n {
            IRFlowNode::Quant(q) => Some(q.clone()),
            _ => None,
        })
        .expect("quant node")
}

/// The bare paper form `quant { … }` parses, lowers, and defaults the backend
/// effect to `quant_sim` with every optional attribute absent.
#[test]
fn bare_quant_block_parses_with_defaults() {
    let src = "flow F(audio_tensor: String) -> String {\n\
                  quant {\n\
                     let surrogate = audio_tensor\n\
                     probe surrogate\n\
                  }\n\
                  return surrogate\n\
               }";
    let q = quant_of(&ir_of(src), "F");
    assert_eq!(q.effect, "quant_sim", "bare quant defaults to the quant_sim backend (D1/D9)");
    assert!(q.encoding.is_none(), "no encoding header ⇒ None (compiler default)");
    assert!(q.observable.is_none());
    assert!(q.qubits.is_none());
    assert!(q.depth.is_none());
    assert!(q.bandwidth.is_none());
}

/// The body lowers to REAL nested flow-IR (not skipped tokens) so v2.4.0's
/// Continuous Type Invariant scans actual AST.
#[test]
fn quant_body_lowers_to_nested_flow_ir() {
    let src = "flow F(audio_tensor: String) -> String {\n\
                  quant {\n\
                     let surrogate = audio_tensor\n\
                     probe surrogate\n\
                  }\n\
                  return surrogate\n\
               }";
    let q = quant_of(&ir_of(src), "F");
    assert_eq!(q.body.len(), 2, "the two body statements lower into the quant IR body");
    assert!(matches!(q.body[0], IRFlowNode::Let(_)), "first body node is the let binding");
    assert!(matches!(q.body[1], IRFlowNode::Probe(_)), "second body node is the probe step");
}

/// The full attribute header parses every key and lowers it verbatim.
#[test]
fn quant_full_header_parses_all_attributes() {
    let src = "flow F(t: String) -> String {\n\
                  quant(encoding: amplitude, observable: EnergyHamiltonian, qubits: 10, depth: 4, bandwidth: 0.5, backend: qpu_native) {\n\
                     probe t\n\
                  }\n\
                  return t\n\
               }";
    let q = quant_of(&ir_of(src), "F");
    assert_eq!(q.encoding.as_deref(), Some("amplitude"));
    assert_eq!(q.observable.as_deref(), Some("EnergyHamiltonian"));
    assert_eq!(q.qubits, Some(10));
    assert_eq!(q.depth, Some(4));
    assert_eq!(q.bandwidth, Some(0.5));
    assert_eq!(q.effect, "qpu_native", "backend: qpu_native overrides the default effect");
}

/// Attributes are order-free and tolerate a trailing comma.
#[test]
fn quant_header_is_order_free_with_trailing_comma() {
    let src = "flow F(t: String) -> String {\n\
                  quant(qubits: 8, encoding: angle,) {\n\
                     probe t\n\
                  }\n\
                  return t\n\
               }";
    let q = quant_of(&ir_of(src), "F");
    assert_eq!(q.qubits, Some(8));
    assert_eq!(q.encoding.as_deref(), Some("angle"));
    assert_eq!(q.effect, "quant_sim", "no backend key ⇒ default quant_sim");
}

/// An empty `quant {}` is valid (degenerate no-op block).
#[test]
fn empty_quant_block_is_valid() {
    let src = "flow F() -> String {\n\
                  quant { }\n\
                  return \"ok\"\n\
               }";
    let q = quant_of(&ir_of(src), "F");
    assert!(q.body.is_empty());
    assert_eq!(q.effect, "quant_sim");
}

/// An unknown attribute is rejected at parse time with an actionable message.
#[test]
fn unknown_quant_attribute_is_rejected() {
    let src = "flow F(t: String) -> String {\n\
                  quant(spin: 3) {\n\
                     probe t\n\
                  }\n\
                  return t\n\
               }";
    let err = parse_result(src).expect_err("unknown attribute must be a parse error");
    assert!(
        err.contains("Unknown `quant` attribute") && err.contains("spin"),
        "error should name the offending attribute, got: {err}"
    );
    // The "expected one of …" help text must enumerate EVERY attribute the
    // parser actually accepts — including `reupload` (v2.23.0). A drift here
    // is exactly the brief #29 failure mode: a parser that accepts an attribute
    // its own diagnostic denies exists.
    assert!(
        err.contains("reupload"),
        "the unknown-attribute help must list `reupload` (parser accepts it), got: {err}"
    );
}

/// A bare `quant {}` serializes with the optional attributes serde-elided
/// (diff-stable JSON) while always carrying its `node_type` + `effect`.
#[test]
fn bare_quant_serializes_without_optional_attrs() {
    let src = "flow F() -> String {\n\
                  quant { }\n\
                  return \"ok\"\n\
               }";
    let q = quant_of(&ir_of(src), "F");
    let json = serde_json::to_value(&q).expect("serialize IRQuant");
    assert_eq!(json["node_type"], "quant");
    assert_eq!(json["effect"], "quant_sim");
    assert!(json.get("encoding").is_none(), "absent encoding is serde-elided");
    assert!(json.get("observable").is_none());
    assert!(json.get("qubits").is_none());
    assert!(json.get("bandwidth").is_none());
}
