//! v2.66.0 — `notify`: grammar + the three laws of governed human
//! notification. See `docs/cycle/governed_human_notification.md`.
//!
//! Pinned properties:
//! 1. The declaration parses (channel / `to: secret(...)` / template /
//!    window / provenance / effects) and lowers to `IRProgram.notifications`.
//! 2. **axon-T933** — the evidence barrier: `cleared` + `${ref}` slots
//!    refuses; vouched (`believe { … }`) passes; literal-only cleared passes.
//! 3. **axon-T934** — structure: closed channel catalog; a LITERAL recipient
//!    refuses (PII never rides source); template + `web` effect required.
//! 4. **axon-T935** — the mandatory window (unbounded interruption refused).

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

fn ir_json(src: &str) -> String {
    let prog = parse(src);
    let ir = IRGenerator::new().generate(&prog);
    serde_json::to_string(&ir).expect("serialize IR")
}

const GOOD: &str = r#"
notify LowSales {
    channel:    sms
    to:         secret(ops.oncall_phone)
    template:   "Ventas 7d: ${resumen}"
    window:     4h
    provenance: attached
    effects:    <web>
}
"#;

// ── 1. Grammar + IR ──────────────────────────────────────────────────────────

#[test]
fn parses_and_lowers_clean() {
    let errs = check_errors(GOOD);
    assert!(
        !errs.iter().any(|e| e.contains("axon-T93")),
        "the governed form checks clean: {errs:?}"
    );
    let json = ir_json(GOOD);
    assert!(json.contains("\"notifications\""), "{json}");
    assert!(json.contains("\"channel\":\"sms\""));
    assert!(
        json.contains("\"to_secret\":\"ops.oncall_phone\""),
        "the CLASS rides the IR — never a number: {json}"
    );
    assert!(json.contains("\"window\":\"4h\""));
}

#[test]
fn all_three_channels_parse() {
    for ch in ["sms", "whatsapp", "telegram"] {
        let src = format!(
            "notify N {{ channel: {ch}\n to: secret(a.b)\n template: \"hola\"\n window: 1d\n effects: <web> }}"
        );
        let errs = check_errors(&src);
        assert!(
            !errs.iter().any(|e| e.contains("axon-T934")),
            "{ch}: {errs:?}"
        );
    }
}

// ── 2. axon-T934 — structure ─────────────────────────────────────────────────

#[test]
fn t934_refuses_a_literal_recipient_teaching_custody() {
    let src = r#"
notify N { channel: sms  to: "+573001234567"  template: "x"  window: 1h  effects: <web> }
"#;
    let errs = check_errors(src);
    let t934 = errs
        .iter()
        .find(|e| e.contains("axon-T934") && e.contains("literal recipient"))
        .unwrap_or_else(|| panic!("a phone number in source must refuse: {errs:?}"));
    assert!(
        t934.contains("secret(ops.oncall_phone)"),
        "the refusal teaches the custody form: {t934}"
    );
}

#[test]
fn t934_refuses_unknown_channel_missing_template_and_missing_web() {
    let src = "notify N { channel: pigeon\n to: secret(a.b)\n window: 1h }";
    let errs: Vec<String> = check_errors(src)
        .into_iter()
        .filter(|e| e.contains("axon-T934"))
        .collect();
    assert!(errs.iter().any(|e| e.contains("pigeon")), "{errs:?}");
    assert!(errs.iter().any(|e| e.contains("no `template:`")), "{errs:?}");
    assert!(errs.iter().any(|e| e.contains("no `web` effect")), "{errs:?}");
}

// ── 3. axon-T935 — the mandatory window ──────────────────────────────────────

#[test]
fn t935_refuses_missing_zero_and_malformed_windows() {
    for w in ["", "0h", "abc", "4x"] {
        let src = format!(
            "notify N {{ channel: sms\n to: secret(a.b)\n template: \"x\"\n {}effects: <web> }}",
            if w.is_empty() { String::new() } else { format!("window: {w}\n ") }
        );
        let errs = check_errors(&src);
        assert!(
            errs.iter().any(|e| e.contains("axon-T935")),
            "window `{w}` must refuse: {errs:?}"
        );
    }
}

// ── 4. axon-T933 — the evidence barrier ──────────────────────────────────────

#[test]
fn t933_cleared_with_flow_slots_refuses() {
    let src = r#"
notify N { channel: sms  to: secret(a.b)  template: "alerta: ${valor}"  window: 1h
    provenance: cleared  effects: <web> }
"#;
    let errs = check_errors(src);
    let t933 = errs
        .iter()
        .find(|e| e.contains("axon-T933"))
        .unwrap_or_else(|| panic!("cleared + slots must refuse: {errs:?}"));
    assert!(
        t933.contains("human") || t933.contains("HUMAN"),
        "the message names the stakes: {t933}"
    );
}

#[test]
fn t933_cleared_under_believe_passes_and_literal_only_passes() {
    let src = r#"
believe {
    notify V { channel: sms  to: secret(a.b)  template: "verificado: ${valor}"  window: 1h
        provenance: cleared  effects: <web> }
}
"#;
    let errs = check_errors(src);
    assert!(
        !errs.iter().any(|e| e.contains("axon-T933")),
        "vouched must pass: {errs:?}"
    );
    let src = r#"
notify L { channel: sms  to: secret(a.b)  template: "mantenimiento programado 22:00"  window: 1d
    provenance: cleared  effects: <web> }
"#;
    let errs = check_errors(src);
    assert!(
        !errs.iter().any(|e| e.contains("axon-T933")),
        "a literal-only cleared notify launders nothing: {errs:?}"
    );
}

#[test]
fn ir_records_the_enclosing_epistemic_mode() {
    let src = r#"
believe {
    notify V { channel: sms  to: secret(a.b)  template: "ok: ${v}"  window: 1h
        provenance: cleared  effects: <web> }
}
"#;
    let json = ir_json(src);
    assert!(
        json.contains("\"epistemic_mode\":\"believe\""),
        "the vouch re-derives at deploy (PCC): {json}"
    );
}
