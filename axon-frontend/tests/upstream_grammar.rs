//! v2.37.0 — grammar + AST + IR + type-checker for `upstream`
//! (the outbound vendor connection — `docs/cycle/upstream_design.md`).
//!
//! Pinned properties:
//! 1. A full `upstream` parses into `UpstreamDefinition` (every field).
//! 2. The v2.37.0 preset-instantiation form (`from Preset@v1`) parses.
//! 3. It lowers to `IRUpstream`; absent optionals are ELIDED from the JSON.
//! 4. **IR-SHA invariance**: a program with no `upstream` serializes with no
//! `upstreams` key — byte-identical to pre-v2.37.0 IR (v2.33.0 discipline).
//! 5. A well-formed upstream produces zero diagnostics.
//! 6. **axon-T849** — partial projection (missing rule), ambiguous inbound
//!    discriminators, and rules naming messages the role never exchanges.
//! 7. **axon-T850** — `resolve:`/`secret:` must be config keys (the
//!    compile-time `SecretKeyPolicy` mirror; a URL literal cannot compile).
//! 8. **axon-T851** — unknown session / unknown role.

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

/// The canonical cascaded-STT shape from the design doc section 1.
const DEEPGRAM: &str = r#"
session SttDialogue {
    axon:   [ send AudioChunk, receive Transcript, loop ]
    vendor: [ receive AudioChunk, send Transcript, loop ]
}

upstream DeepgramSTT {
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
    reconnect: { backoff_ms: 500, max_attempts: 5, on_exhausted: fail }
    overflow: drop_oldest
}
"#;

fn first_upstream(prog: &axon_frontend::ast::Program) -> &axon_frontend::ast::UpstreamDefinition {
    prog.declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Upstream(u) => Some(u),
            _ => None,
        })
        .expect("no upstream declaration")
}

#[test]
fn upstream_parses_into_ast() {
    let prog = parse(DEEPGRAM);
    let u = first_upstream(&prog);

    assert_eq!(u.name, "DeepgramSTT");
    assert_eq!(u.transport, "websocket");
    assert_eq!(u.protocol, "SttDialogue");
    assert_eq!(u.role, "axon");
    assert_eq!(u.resolve, "upstream.deepgram.url");
    assert_eq!(u.secret, "upstream.deepgram.api_key");
    assert_eq!(u.auth_kind, "header");
    assert_eq!(u.auth_name.as_deref(), Some("Authorization"));
    assert_eq!(u.auth_prefix.as_deref(), Some("Token "));
    assert_eq!(u.overflow.as_deref(), Some("drop_oldest"));
    assert!(u.preset.is_none(), "hand-written, not preset-expanded");

    assert_eq!(u.map.len(), 2);
    let audio = &u.map[0];
    assert_eq!((audio.direction.as_str(), audio.message.as_str(), audio.framing.as_str()), ("send", "AudioChunk", "binary"));
    let transcript = &u.map[1];
    assert_eq!((transcript.direction.as_str(), transcript.framing.as_str()), ("receive", "json"));
    assert_eq!(transcript.when_field.as_deref(), Some("type"));
    assert_eq!(transcript.when_value.as_deref(), Some("Results"));

    let rc = u.reconnect.as_ref().expect("reconnect policy");
    assert_eq!((rc.backoff_ms, rc.max_attempts, rc.on_exhausted.as_str()), (500, 5, "fail"));
}

#[test]
fn preset_instantiation_form_parses() {
    let prog = parse(
        r#"
upstream MySTT from DeepgramSTT@v1 {
    secret: upstream.deepgram.api_key
}
"#,
    );
    let u = first_upstream(&prog);
    assert_eq!(u.name, "MySTT");
    assert_eq!(u.preset.as_deref(), Some("DeepgramSTT@v1"));
    assert_eq!(u.secret, "upstream.deepgram.api_key");
}

#[test]
fn upstream_lowers_to_ir_with_elided_optionals() {
    let prog = parse(DEEPGRAM);
    let ir = IRGenerator::new().generate(&prog);
    let u = ir.upstreams.first().expect("no upstream in IR");

    assert_eq!(u.node_type, "upstream");
    assert_eq!(u.name, "DeepgramSTT");
    assert_eq!(u.map.len(), 2);

    let json = serde_json::to_string(u).expect("serialize");
    // Present fields ride the IR…
    assert!(json.contains("\"auth_prefix\":\"Token \""), "got: {json}");
    assert!(json.contains("\"when_value\":\"Results\""), "got: {json}");
    // …absent optionals are ELIDED (send-binary rule has no tag/when;
    // no preset, no backpressure_credit on this declaration).
    assert!(!json.contains("\"tag\""), "absent tag must elide: {json}");
    assert!(!json.contains("\"preset\""), "absent preset must elide: {json}");
    assert!(
        !json.contains("\"backpressure_credit\""),
        "absent credit must elide: {json}"
    );
}

#[test]
fn upstream_less_program_has_no_ir_drift() {
    let prog = parse(
        r#"
session Ping {
    a: [ send Msg, end ]
    b: [ receive Msg, end ]
}
socket PingWS { protocol: Ping }
"#,
    );
    let ir = IRGenerator::new().generate(&prog);
    let json = serde_json::to_string(&ir).expect("serialize");
    assert!(
        !json.contains("\"upstreams\""),
        "no upstream ⇒ no `upstreams` key in IR JSON (IR-SHA stability): …"
    );
}

#[test]
fn well_formed_upstream_produces_no_diagnostics() {
    let errors = check_errors(DEEPGRAM);
    let mine: Vec<_> = errors
        .iter()
        .filter(|m| m.contains("Upstream") || m.contains("axon-T85") || m.contains("axon-T849"))
        .collect();
    assert!(mine.is_empty(), "expected clean check, got: {mine:?}");
}

#[test]
fn t849_missing_projection_rule_is_an_error() {
    // Transcript has no receive rule → partial transcoding.
    let src = DEEPGRAM.replace("receive Transcript as json when \"type\" = \"Results\",", "");
    let errors = check_errors(&src);
    assert!(
        errors.iter().any(|m| m.contains("axon-T849") && m.contains("Transcript")),
        "partial projection must be axon-T849, got: {errors:?}"
    );
}

#[test]
fn t849_ambiguous_receive_discriminators_are_an_error() {
    let src = r#"
session Duo {
    axon:   [ receive A, receive B, end ]
    vendor: [ send A, send B, end ]
}
upstream V {
    transport: websocket
    protocol: Duo
    role: axon
    resolve: upstream.v.url
    secret: upstream.v.api_key
    auth: query("token")
    map: [
        receive A as json when "kind" = "x",
        receive B as json when "kind" = "x",
    ]
}
"#;
    let errors = check_errors(src);
    assert!(
        errors.iter().any(|m| m.contains("axon-T849") && m.contains("discriminator")),
        "ambiguous inbound dispatch must be axon-T849, got: {errors:?}"
    );
}

#[test]
fn t849_rule_for_unknown_message_is_an_error() {
    let src = DEEPGRAM.replace(
        "send AudioChunk as binary,",
        "send AudioChunk as binary, send Phantom as json,",
    );
    let errors = check_errors(&src);
    assert!(
        errors.iter().any(|m| m.contains("axon-T849") && m.contains("Phantom")),
        "a rule naming a message the role never sends must be axon-T849, got: {errors:?}"
    );
}

#[test]
fn t850_url_literal_in_resolve_is_an_error() {
    // A quoted URL parses as a StringLit value, but the charset law
    // (compile-time SecretKeyPolicy mirror) rejects it — config, not code.
    let src = DEEPGRAM.replace("resolve: upstream.deepgram.url", "resolve: \"wss://api.deepgram.com/v1/listen\"");
    let errors = check_errors(&src);
    assert!(
        errors.iter().any(|m| m.contains("axon-T850") && m.contains("resolve")),
        "a URL literal in `resolve:` must be axon-T850, got: {errors:?}"
    );
}

#[test]
fn t850_bad_charset_secret_key_is_an_error() {
    let src = DEEPGRAM.replace("secret: upstream.deepgram.api_key", "secret: Upstream.Deepgram.ApiKey");
    let errors = check_errors(&src);
    assert!(
        errors.iter().any(|m| m.contains("axon-T850") && m.contains("secret")),
        "an uppercase key (production custody would reject it) must be axon-T850, got: {errors:?}"
    );
}

#[test]
fn t851_unknown_session_and_role_are_errors() {
    let unknown_session = DEEPGRAM.replace("protocol: SttDialogue", "protocol: NoSuchSession");
    let errors = check_errors(&unknown_session);
    assert!(
        errors.iter().any(|m| m.contains("axon-T851") && m.contains("NoSuchSession")),
        "unknown session must be axon-T851, got: {errors:?}"
    );

    let unknown_role = DEEPGRAM.replace("role: axon\n", "role: carrier\n");
    let errors = check_errors(&unknown_role);
    assert!(
        errors.iter().any(|m| m.contains("axon-T851") && m.contains("carrier")),
        "unknown role must be axon-T851, got: {errors:?}"
    );
}

#[test]
fn leading_loop_session_warns_w012_and_never_hangs() {
    // Pre-v2.37.0 idiom: `[ loop, … ]` lowers to the unguarded μX.X — duality
    // and credit hold VACUOUSLY, and handing μX.X to the v2.3.0 discharge
    // historically risked non-termination. Law: warn (axon-W012), skip the
    // analyses, terminate. This test hanging IS the regression.
    let src = r#"
session P {
    a: [ loop, send M, end ]
    b: [ loop, receive M, end ]
}
socket S { protocol: P  backpressure: credit(4) }
"#;
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    let (errors, warnings) = TypeChecker::new(&prog).check_with_warnings();
    assert!(
        warnings.iter().any(|w| w.message.contains("axon-W012") && w.message.contains("vacuous")),
        "leading loop must warn W012, got warnings: {:?}",
        warnings.iter().map(|w| &w.message).collect::<Vec<_>>()
    );
    assert!(
        !errors.iter().any(|e| e.message.contains("credit")),
        "the degenerate type must be SKIPPED by the discharge, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

#[test]
fn interrupt_session_messages_are_seen_by_totality() {
    // v2.36.0 interop: a message exchanged only inside an interrupt
    // handler still crosses the wire — T849 must demand a rule for it.
    let src = r#"
session Interruptible {
    axon: [
        interrupt { send Audio } on CallerSpeech as c resumable { send Flush, resume }
    ]
    vendor: [
        interrupt { receive Audio } on CallerSpeech as c resumable { receive Flush, resume }
    ]
}
upstream V {
    transport: websocket
    protocol: Interruptible
    role: axon
    resolve: upstream.v.url
    secret: upstream.v.api_key
    auth: signed_url
    map: [ send Audio as binary ]
}
"#;
    let errors = check_errors(src);
    assert!(
        errors.iter().any(|m| m.contains("axon-T849") && m.contains("Flush")),
        "handler-only messages must be covered by the projection, got: {errors:?}"
    );
}
