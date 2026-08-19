//! v2.69.0 — `upstream { resource: R }`: the client leg rides a governed
//! channel, closing the LAST address-bearing declaration outside the
//! resource model.
//!
//! Pinned properties:
//! 1. `resource:` parses into `UpstreamDefinition.resource_ref`.
//! 2. **axon-T951 (XOR)** — `resource:` beside `resolve:` is refused: the
//! channel's address stated twice is the v2.67.0 islands defect.
//! 3. **axon-T951 (reference)** — the name must resolve to a DECLARED
//!    `resource` (undeclared, and wrong-kind, both refused — the T950 law).
//! 4. A resourced upstream needs NO `resolve:` (the T850 "no resolve" error
//!    must NOT fire — the address derives).
//! 5. **Derivation at lowering** — `IRUpstream.resolve` carries the
//!    resource's `endpoint` config key and `IRUpstream.capacity` its bound,
//!    REGARDLESS of declaration order (the Phase 0 pre-pass, so no dial
//!    path can meet an unstamped artifact).
//! 6. **IR-SHA stability** — an un-resourced upstream serializes with no
//! `resource_ref` / `capacity` keys: every pre-v2.69.0 program is
//!    byte-identical.
//! 7. **axon-T945** — an upstream that rides a resource is a HOLDER: an
//!    `affine` resource shared between an upstream and a tool is a breach
//! (without this, v2.69.0 would quietly re-open the hole v2.67.0 closed).

use axon_frontend::ir_generator::IRGenerator;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn parse(src: &str) -> axon_frontend::ast::Program {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

fn check_errors(src: &str) -> Vec<String> {
    let prog = parse(src);
    TypeChecker::new(&prog)
        .check()
        .iter()
        .map(|e| e.message.clone())
        .collect()
}

const SESSION: &str = r#"
session SttDialogue {
    axon:   [ send AudioChunk, receive Transcript, loop ]
    vendor: [ receive AudioChunk, send Transcript, loop ]
}
"#;

const RESOURCE: &str = r#"
resource SttVendor {
    kind: https
    endpoint: upstream.deepgram.url
    capacity: 3
    lifetime: affine
}
"#;

/// The v2.69.0 shape: the upstream names the resource; NO `resolve:`.
fn resourced_upstream(extra: &str) -> String {
    format!(
        r#"{SESSION}
{RESOURCE}
upstream DeepgramSTT {{
    transport: websocket
    protocol: SttDialogue
    role: axon
    resource: SttVendor
    {extra}
    secret: upstream.deepgram.api_key
    auth: header("Authorization", "Token ")
    map: [
        send AudioChunk as binary,
        receive Transcript as json when "type" = "Results",
    ]
}}
"#
    )
}

#[test]
fn resource_parses_into_ast() {
    let prog = parse(&resourced_upstream(""));
    let u = prog
        .declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Upstream(u) => Some(u),
            _ => None,
        })
        .expect("upstream");
    assert_eq!(u.resource_ref, "SttVendor");
    assert!(u.resolve.is_empty(), "no resolve: was declared");
}

#[test]
fn t951_resource_beside_resolve_is_refused() {
    let errors = check_errors(&resourced_upstream("resolve: upstream.deepgram.url"));
    assert!(
        errors.iter().any(|e| e.contains("axon-T951") && e.contains("BOTH")),
        "resource+resolve must be the T951 XOR error, got: {errors:?}"
    );
}

#[test]
fn t951_undeclared_resource_is_refused() {
    let src = resourced_upstream("").replace("resource: SttVendor", "resource: Ghost");
    let errors = check_errors(&src);
    assert!(
        errors.iter().any(|e| e.contains("axon-T951") && e.contains("not declared")),
        "undeclared resource must fail T951, got: {errors:?}"
    );
}

#[test]
fn t951_non_resource_symbol_is_refused() {
    // `SttDialogue` is a session, not a resource.
    let src = resourced_upstream("").replace("resource: SttVendor", "resource: SttDialogue");
    let errors = check_errors(&src);
    assert!(
        errors.iter().any(|e| e.contains("axon-T951") && e.contains("not a resource")),
        "non-resource symbol must fail T951, got: {errors:?}"
    );
}

#[test]
fn resourced_upstream_needs_no_resolve() {
    let errors = check_errors(&resourced_upstream(""));
    assert!(
        !errors.iter().any(|e| e.contains("axon-T850") && e.contains("resolve")),
        "the address derives from the resource — T850 must not demand a `resolve:`, got: {errors:?}"
    );
    // …and the whole program is otherwise clean.
    assert!(
        errors.is_empty(),
        "a well-formed resourced upstream must produce zero diagnostics, got: {errors:?}"
    );
}

#[test]
fn lowering_derives_resolve_and_capacity_from_the_resource() {
    let prog = parse(&resourced_upstream(""));
    let ir = IRGenerator::new().generate(&prog);
    let u = ir.upstreams.first().expect("upstream in IR");
    assert_eq!(
        u.resolve, "upstream.deepgram.url",
        "the dial address must DERIVE from resource.endpoint at lowering"
    );
    assert_eq!(u.resource_ref, "SttVendor");
    assert_eq!(u.capacity, Some(3), "the instance bound must carry resource.capacity");
}

#[test]
fn derivation_is_declaration_order_independent() {
    // Upstream FIRST, resource LAST — the Phase 0 pre-pass must still stamp.
    let src = format!(
        r#"{SESSION}
upstream DeepgramSTT {{
    transport: websocket
    protocol: SttDialogue
    role: axon
    resource: SttVendor
    secret: upstream.deepgram.api_key
    auth: header("Authorization", "Token ")
    map: [
        send AudioChunk as binary,
        receive Transcript as json when "type" = "Results",
    ]
}}
{RESOURCE}
"#
    );
    let prog = parse(&src);
    let ir = IRGenerator::new().generate(&prog);
    let u = ir.upstreams.first().expect("upstream in IR");
    assert_eq!(u.resolve, "upstream.deepgram.url");
    assert_eq!(u.capacity, Some(3));
}

#[test]
fn unresourced_upstream_serializes_without_the_new_keys() {
    // IR-SHA stability: every pre-v2.69.0 upstream is byte-identical.
    let src = format!(
        r#"{SESSION}
upstream DeepgramSTT {{
    transport: websocket
    protocol: SttDialogue
    role: axon
    resolve: upstream.deepgram.url
    secret: upstream.deepgram.api_key
    auth: header("Authorization", "Token ")
    map: [
        send AudioChunk as binary,
        receive Transcript as json when "type" = "Results",
    ]
}}
"#
    );
    let prog = parse(&src);
    let ir = IRGenerator::new().generate(&prog);
    let json = serde_json::to_string(&ir.upstreams[0]).expect("serialize");
    assert!(
        !json.contains("resource_ref") && !json.contains("capacity"),
        "un-resourced upstream must elide the v2.69.0 keys, got: {json}"
    );
}

#[test]
fn t945_upstream_is_a_holder_of_its_resource() {
    // An affine resource named by an upstream AND a tool → sharing breach,
    // and the diagnostic must name the upstream as one of the holders.
    let src = format!(
        r#"{SESSION}
{RESOURCE}
upstream DeepgramSTT {{
    transport: websocket
    protocol: SttDialogue
    role: axon
    resource: SttVendor
    secret: upstream.deepgram.api_key
    auth: header("Authorization", "Token ")
    map: [
        send AudioChunk as binary,
        receive Transcript as json when "type" = "Results",
    ]
}}
tool Transcriber {{
    description: "vendor transcription"
    resource: SttVendor
}}
"#
    );
    let errors = check_errors(&src);
    assert!(
        errors.iter().any(|e| e.contains("axon-T945")
            && e.contains("upstream 'DeepgramSTT'")
            && e.contains("tool 'Transcriber'")),
        "affine sharing between an upstream and a tool must breach T945 naming both, got: {errors:?}"
    );
}
