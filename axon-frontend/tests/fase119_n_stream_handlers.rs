//! §Fase 119.n — **`stream<T> { on_chunk: … on_complete: … }` is REAL.**
//!
//! **What was wrong, measured before this landed.** README block 15 publishes
//!
//! ```text
//! step Stream {
//!     stream<QuoteData> {
//!         on_chunk:    { probe chunk for [symbol, price, volume]
//!                        output: QuoteSnapshot }
//!         on_complete: { validate QuoteSnapshot against: MarketSchema
//!                        output: VerifiedQuote }
//!     }
//! }
//! ```
//!
//! and that block **compiled with `0 errors` while doing nothing at all**. The
//! step-body parser routed `stream` to `skip_flow_step_structural`, so the whole
//! construct — the `probe`, the `validate`, and BOTH `output:` declarations —
//! was discarded at parse time. `step Stream` reached the dispatcher with
//! `pix_ops=0`, `ask=""`, `output=""`: an entirely EMPTY step, whose
//! `Stream.output` the next step then reasoned over. The block had left the §117
//! ledger on the strength of compiling.
//!
//! At FLOW level the same body was a hard parse error
//! (`Unexpected token in flow body: 'on_chunk'`), because §111.e had given
//! `StreamBlock` a `body: Vec<FlowStep>` — a shape **no published block and no
//! paper writes**. `fase_23_algebraic_effects_runtime.md` §3.7 specifies
//! `stream<τ> { on_chunk: B₁ on_complete: B₂ }`, and its D8 promises *"backward
//! compat 100%, cero cambios en `.axon` source files de adopters"*.
//!
//! So `stream`'s `advertised.rs` attestation was the §119.f.8 `reason` defect
//! exactly: `Real`, citing a gate that exercised an engine no published program
//! could reach. This file is the replacement proof, and it walks **source →
//! AST → IR** rather than naming a function.
//!
//! **What this pins.**
//!   - the published surface parses at BOTH positions (step body and flow body);
//!   - the `<T>` chunk type REACHES the IR — it used to be eaten by the
//!     "tolerate the pre-111 form" skip loop, so the one piece of type
//!     information the author wrote was discarded;
//!   - a handler arm is a STEP body, so `output:` lands (it has no flow-level
//!     position — parsing the arm as a flow body would reject block 15);
//!   - §111.e's `stream { <steps> }` body form still parses — this is additive;
//!   - the handler catalog is CLOSED: an unknown or duplicated arm is REFUSED in
//!     writing, because a skipped handler removes the processing the author
//!     wrote and silence in that direction is indistinguishable from a stream
//!     that had nothing to do;
//!   - the IR stays byte-identical for a program that uses none of this.

use axon_frontend::ast::{Declaration, FlowStep, Program, StreamBlock};
use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

fn parse(src: &str) -> Result<Program, axon_frontend::parser::ParseError> {
    let tokens = Lexer::new(src, "t.axon").tokenize().expect("lex");
    Parser::new(tokens).parse()
}

fn ir_json(src: &str) -> String {
    let program = parse(src).expect("parse");
    serde_json::to_string_pretty(&IRGenerator::new().generate(&program)).expect("json")
}

/// The `stream` block hanging off the first step that declares one.
fn step_stream(program: &Program) -> &StreamBlock {
    for d in &program.declarations {
        if let Declaration::Flow(f) = d {
            for s in &f.body {
                if let FlowStep::Step(st) = s {
                    if let Some(sb) = &st.stream {
                        return sb;
                    }
                }
            }
        }
    }
    panic!("no step-body `stream` block found");
}

/// README block 15, verbatim.
const README_BLOCK_15: &str = r#"
tool MarketFeed {
    timeout: 5s
    effects: <io, network, epistemic:speculate>
}

flow MonitorMarket(sector: String) -> MarketReport {
    step Stream {
        stream<QuoteData> {
            on_chunk: {
                probe chunk for [symbol, price, volume]
                output: QuoteSnapshot
            }
            on_complete: {
                validate QuoteSnapshot against: MarketSchema
                output: VerifiedQuote
            }
        }
    }
    step Analyze {
        reason {
            given: Stream.output
            ask: "Identify anomalous price movements"
            depth: 2
        }
        output: MarketReport
    }
}
"#;

// ── The published surface ────────────────────────────────────────────────

/// The defect, stated as the thing that must never come back: block 15's first
/// step must not be EMPTY.
#[test]
fn readme_block_15_step_is_no_longer_discarded() {
    let program = parse(README_BLOCK_15).expect("README block 15 parses");
    let sb = step_stream(&program);

    assert_eq!(
        sb.chunk_type, "QuoteData",
        "the `<T>` chunk type must reach the AST — the skip loop used to eat it"
    );
    assert!(
        sb.on_chunk.is_some(),
        "`on_chunk` is the handler the whole example is built around"
    );
    assert!(sb.on_complete.is_some(), "`on_complete` must survive too");
}

/// The arm is a STEP body, which is why `output:` has somewhere to land.
#[test]
fn handler_arms_carry_their_statements_and_their_output() {
    let program = parse(README_BLOCK_15).expect("parse");
    let sb = step_stream(&program);

    let on_chunk = sb.on_chunk.as_ref().expect("on_chunk");
    assert_eq!(
        on_chunk.output_type, "QuoteSnapshot",
        "`output:` inside the arm is the arm's result type"
    );
    assert!(
        on_chunk
            .pix_ops
            .iter()
            .any(|op| matches!(op, FlowStep::Probe(_))),
        "the `probe chunk for […]` statement must reach the arm's body, not be skipped"
    );

    let on_complete = sb.on_complete.as_ref().expect("on_complete");
    assert_eq!(on_complete.output_type, "VerifiedQuote");
    assert!(
        on_complete
            .pix_ops
            .iter()
            .any(|op| matches!(op, FlowStep::Validate(_))),
        "the `validate … against:` statement must reach the arm's body"
    );
}

/// The FLOW-level position: this exact body used to be
/// `Unexpected token in flow body: 'on_chunk'`.
#[test]
fn the_published_body_parses_at_flow_level_too() {
    let src = r#"
flow F(x: String) -> Text {
    stream<Token> {
        on_chunk: {
            probe chunk for [value]
            output: Text
        }
    }
}
"#;
    let program = parse(src).expect("flow-level `stream<T>` with the published body must parse");
    for d in &program.declarations {
        if let Declaration::Flow(f) = d {
            for s in &f.body {
                if let FlowStep::Stream(sb) = s {
                    assert_eq!(sb.chunk_type, "Token");
                    assert!(sb.on_chunk.is_some());
                    return;
                }
            }
        }
    }
    panic!("no flow-level stream block");
}

// ── Additivity ───────────────────────────────────────────────────────────

/// §111.e's form still parses. This landing is additive, not a replacement.
#[test]
fn the_fase_111e_body_form_still_parses() {
    let src = r#"
flow F(x: String) -> Text {
    stream {
        step Inner {
            ask: "hi"
            output: Text
        }
    }
}
"#;
    let program = parse(src).expect("the §111.e body form must keep parsing");
    for d in &program.declarations {
        if let Declaration::Flow(f) = d {
            for s in &f.body {
                if let FlowStep::Stream(sb) = s {
                    assert_eq!(sb.body.len(), 1, "the §111.e body still lowers");
                    assert!(sb.on_chunk.is_none());
                    assert!(sb.chunk_type.is_empty());
                    return;
                }
            }
        }
    }
    panic!("no stream block");
}

/// The §68.b/§91.a discipline: a program that uses none of the new grammar must
/// serialise byte-identically, so no existing artifact's IR-SHA moves.
#[test]
fn absent_handlers_are_elided_from_the_ir() {
    let src = r#"
flow F(x: String) -> Text {
    step S {
        ask: "hello"
        output: Text
    }
}
"#;
    let json = ir_json(src);
    assert!(
        !json.contains("on_chunk"),
        "an absent handler must not appear in the IR: {json}"
    );
    assert!(
        !json.contains("chunk_type"),
        "an absent chunk type must not appear in the IR: {json}"
    );
    assert!(
        !json.contains("\"stream\""),
        "a step with no stream block must not carry the key: {json}"
    );
}

/// The handlers must reach the IR — an AST field nothing lowers is the §111
/// defect this fase exists to end.
#[test]
fn the_handlers_and_chunk_type_reach_the_ir() {
    let json = ir_json(README_BLOCK_15);
    assert!(json.contains("\"chunk_type\": \"QuoteData\""), "{json}");
    assert!(json.contains("on_chunk"), "{json}");
    assert!(json.contains("on_complete"), "{json}");
    assert!(
        json.contains("QuoteSnapshot"),
        "the arm's own output type must lower: {json}"
    );
}

// ── The catalog is CLOSED ────────────────────────────────────────────────

/// §119.h.2 — ask which direction the silence fails in. A skipped handler
/// removes the author's processing without a word.
#[test]
fn an_unknown_handler_is_refused_and_names_the_catalog() {
    let src = r#"
flow F(x: String) -> Text {
    step S {
        stream<Token> {
            on_chunkk: {
                output: Text
            }
        }
    }
}
"#;
    let err = parse(src).expect_err("a mis-spelled handler must not be silently skipped");
    assert!(
        err.message.contains("on_chunkk"),
        "the diagnostic must name the offending key: {}",
        err.message
    );
    assert!(
        err.message.contains("on_chunk") && err.message.contains("on_complete"),
        "and list what IS accepted: {}",
        err.message
    );
}

#[test]
fn a_duplicate_handler_is_refused() {
    let src = r#"
flow F(x: String) -> Text {
    step S {
        stream<Token> {
            on_chunk: { output: Text }
            on_chunk: { output: Text }
        }
    }
}
"#;
    let err = parse(src).expect_err("two handlers for one edge have no defined composition");
    assert!(
        err.message.contains("on_chunk"),
        "diagnostic: {}",
        err.message
    );
}

#[test]
fn two_stream_blocks_on_one_step_are_refused() {
    let src = r#"
flow F(x: String) -> Text {
    step S {
        stream<Token> { on_chunk: { output: Text } }
        stream<Token> { on_chunk: { output: Text } }
    }
}
"#;
    let err = parse(src).expect_err("a step has ONE output stream");
    assert!(
        err.message.contains("stream"),
        "diagnostic: {}",
        err.message
    );
}
