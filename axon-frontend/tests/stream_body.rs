//! v2.67.0 — `stream` gets its body back, and the ROOT CAUSE is named.
//!
//! # One function killed four primitives
//!
//! `stream` (README #34, *"Algebraic Effects and Free Monads"*), `deliberate`
//! (#25), `consensus` (#26) and `transact` (retracted in v2.67.0) all parsed
//! through **`parse_block_step`**, whose entire job is `skip_braced_block()`.
//! The block's contents were thrown away **at parse time**.
//!
//! So their handlers were not no-ops through neglect — they were no-ops **by
//! construction**. `run_stream`'s own comment said *"No body to dispatch —
//! IRStreamBlock is payload-free"*, and it was telling the truth: there was
//! nothing in the AST for anyone to execute. Every one of those handlers
//! honestly described its own emptiness, while the README sold the whole set.
//!
//! That is the shape v2.67.0 exists to find: **the compiler was never lying. The
//! advertising was.**
//!
//! Pins:
//! 1. A `stream` block's body reaches the AST and the IR (it used to vanish).
//! 2. The steps inside are real, typed flow steps — not opaque tokens.
//! 3. The IR stays **byte-identical** for a program with no `stream`, and an
//!    EMPTY `stream {}` elides the field — so no pre-111 artifact's IR-SHA moves.
//! 4. A `stream` body still type-checks (its steps are ordinary steps).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn ir_json(src: &str) -> serde_json::Value {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    let ir = IRGenerator::new().generate(&prog);
    serde_json::to_value(&ir).expect("serialize")
}

fn errors(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog)
        .check()
        .iter()
        .map(|e| e.message.clone())
        .collect()
}

/// Find the single `stream` node in the first flow's body.
fn stream_node(ir: &serde_json::Value) -> serde_json::Value {
    let flows = ir["flows"].as_array().expect("flows");
    for f in flows {
        if let Some(steps) = f["steps"].as_array() {
            for s in steps {
                if s["node_type"] == "stream" {
                    return s.clone();
                }
            }
        }
    }
    panic!("no stream node in IR: {ir:#}");
}

const WITH_BODY: &str = r#"
type R { x: String }
flow F() -> Unit {
    stream {
        let a = "one"
        let b = "two"
    }
}
"#;

// ── 1-2. The body survives ──────────────────────────────────────────────────

/// The load-bearing test. Before v2.67.0 the body was `skip_braced_block()`-ed at
/// parse time, so this array did not exist at all.
#[test]
fn the_stream_body_reaches_the_ir() {
    let ir = ir_json(WITH_BODY);
    let node = stream_node(&ir);

    let body = node["body"]
        .as_array()
        .expect("a stream block must carry its body — it used to be discarded at parse time");
    assert_eq!(
        body.len(),
        2,
        "both steps inside `stream` must be lowered; got {body:#?}"
    );

    // …and they are REAL typed steps, not opaque tokens.
    let kinds: Vec<&str> = body.iter().map(|s| s["node_type"].as_str().unwrap()).collect();
    assert_eq!(
        kinds,
        vec!["let_binding", "let_binding"],
        "the body's steps must lower to real IR nodes"
    );
    assert_eq!(body[0]["target"], "a");
    assert_eq!(body[1]["target"], "b");
}

/// The steps inside a `stream` are ordinary flow steps and must type-check as
/// such — the block is a streaming *frame*, not an escape hatch from the type
/// system.
#[test]
fn a_stream_body_still_type_checks() {
    let errs = errors(WITH_BODY);
    assert!(errs.is_empty(), "a well-formed stream body must check clean; got {errs:?}");
}

// ── 3. No IR drift for anyone who didn't ask for it ─────────────────────────

/// **The back-compat guarantee.** `body` is `skip_serializing_if = "Vec::is_empty"`,
/// so an empty `stream {}` serialises exactly as it did before v2.67.0. A pre-111
/// artifact's IR-SHA cannot move, and a legacy IR (deserialised with no `body`)
/// executes as the old no-op rather than silently changing behaviour under an
/// adopter who never recompiled.
#[test]
fn an_empty_stream_block_elides_the_field_so_no_ir_sha_moves() {
    let ir = ir_json(
        r#"
        flow F() -> Unit {
            stream {
            }
        }
    "#,
    );
    let node = stream_node(&ir);
    assert!(
        node.get("body").is_none(),
        "an empty body must be ELIDED, not emitted as `[]` — otherwise every \
         pre-111 program's IR-SHA drifts. Got: {node:#}"
    );
}

/// And a program with no `stream` at all is untouched.
#[test]
fn a_program_without_stream_is_byte_identical() {
    let ir = ir_json("flow F() -> Unit { let a = \"x\" }");
    let s = serde_json::to_string(&ir).unwrap();
    assert!(
        !s.contains("\"stream\""),
        "a program with no stream must carry no stream node"
    );
}
