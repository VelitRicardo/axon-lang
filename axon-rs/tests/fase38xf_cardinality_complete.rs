//! §Fase 38.x.f — Cardinality Coverage Complete.
//!
//! This anchor pins the v1.40.0 promotion of the v1.39.0 narrow gate
//! into a full `Cardinality` propagation pass + bilateral coverage +
//! Stream-vs-spatial distinction + branch-disagreement warning +
//! runtime hint enrichment.
//!
//! Twelve §-assertions cover the eight D-letters:
//!
//!   §1  — D1 expand: `for x in xs { … }` tail + singular output → T9XX
//!   §2  — D1 expand: `if/else` branches DISAGREE → W003 emitted
//!   §3  — D1 expand: `if/else` branches AGREE on Singular → no warning
//!   §4  — D1 expand: `if/else` branches AGREE on Plural → no warning
//!   §5  — D3 bilateral: Singular tail + `output: List<T>` → T9XX
//!   §6  — D5 Stream: `output: Stream<T>` + retrieve tail → T9YY
//!   §7  — D5 Stream: `output: Stream<T>` + Stream step tail → no error
//!   §8  — D5 bilateral Stream: `output: T` + Stream tail → T9YY
//!   §9  — D6: `output: Any` accepts disagreed branches (degraded surface)
//!   §10 — D2 runtime: BodyValidationError exposes cardinality fields
//!   §11 — D4 OWASP-safe default + verbose opt-in semantics
//!   §12 — STATIC grep §S: Cardinality enum + infer_flow_tail_cardinality
//!         + emit_pin_acquire surface declarations present.
//!
//! All assertions run WITHOUT external infrastructure (no Postgres
//! needed; the gate is compile-time pure).

use axon::lexer::Lexer;
use axon::parser::Parser;
use axon::type_checker::{TypeChecker, TypeError};

fn check_errors(src: &str) -> Vec<TypeError> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog).check()
}

// ── §1 — D1 expand: for-tail + singular output ──────────────────────

#[test]
fn s1_d1_for_tail_with_singular_output_emits_t9xx() {
    let src = r#"
        type TenantRow { id: Text }
        type TenantList { rows: List<TenantRow> }
        axonstore tenants { backend: in_memory }
        axonendpoint list_summary { public: true
            method: GET
            path: "/api/summary"
            output: FlowEnvelope<TenantRow>
            execute: BuildSummary
        }
        flow BuildSummary() -> Unit {
            for t in tenants {
                step Summarize { reason: "ok" output: TenantRow }
            }
        }
    "#;
    let errs = check_errors(src);
    let t9xx: Vec<&TypeError> = errs.iter()
        .filter(|e| e.message.contains("axon-T9XX"))
        .collect();
    assert!(
        !t9xx.is_empty(),
        "§Fase 38.x.f §1 — a for-loop tail (always plural) + singular \
         endpoint output MUST emit `axon-T9XX`. The v1.39.0 narrow gate \
         missed this; v1.40.0's full propagation catches it. All errors: \
         {errs:#?}"
    );
}

// ── §2 — D1 expand: if/else branches DISAGREE → W003 ────────────────

#[test]
fn s2_d1_if_else_disagree_emits_w003() {
    let src = r#"
        type Report { ok: Bool }
        axonstore data { backend: in_memory }
        axonendpoint evaluate { public: true
            method: GET
            path: "/api/evaluate"
            output: FlowEnvelope<Report>
            execute: Evaluate
        }
        flow Evaluate() -> Unit {
            if condition == "many" {
                retrieve data { where: "1 = 1" as: rows }
            } else {
                step One { reason: "ok" output: Report }
            }
        }
    "#;
    let errs = check_errors(src);
    let w003: Vec<&TypeError> = errs.iter()
        .filter(|e| e.message.contains("axon-W003"))
        .collect();
    assert!(
        !w003.is_empty(),
        "§Fase 38.x.f §2 — if/else branches disagreeing on cardinality \
         (one Singular, one Plural) MUST emit `axon-W003 \
         cardinality_disagreement_in_branches`. All errors: {errs:#?}"
    );
}

// ── §3 — D1 expand: if/else branches AGREE Singular → no warning ────

#[test]
fn s3_d1_if_else_agree_singular_passes() {
    let src = r#"
        type Report { ok: Bool }
        axonendpoint decide { public: true
            method: GET
            path: "/api/decide"
            output: Report
            execute: Decide
        }
        flow Decide() -> Unit {
            if tier == "premium" {
                step Premium { reason: "ok" output: Report }
            } else {
                step Standard { reason: "ok" output: Report }
            }
        }
    "#;
    let errs = check_errors(src);
    let warns: Vec<&TypeError> = errs.iter()
        .filter(|e| {
            e.message.contains("axon-T9XX")
                || e.message.contains("axon-T9YY")
                || e.message.contains("axon-W003")
        })
        .collect();
    assert!(
        warns.is_empty(),
        "§Fase 38.x.f §3 — if/else branches AGREEING on Singular + \
         singular output is well-formed. No cardinality diagnostic \
         should fire. Errors: {warns:#?}"
    );
}

// ── §4 — D1 expand: if/else branches AGREE Plural → no warning ──────

#[test]
fn s4_d1_if_else_agree_plural_passes() {
    let src = r#"
        type Item { id: Text }
        axonstore catalog_a { backend: in_memory }
        axonstore catalog_b { backend: in_memory }
        axonendpoint list_items { public: true
            method: GET
            path: "/api/items"
            output: List<Item>
            execute: ListItems
        }
        flow ListItems() -> Unit {
            if region == "us" {
                retrieve catalog_a { where: "1 = 1" as: rows }
            } else {
                retrieve catalog_b { where: "1 = 1" as: rows }
            }
        }
    "#;
    let errs = check_errors(src);
    let warns: Vec<&TypeError> = errs.iter()
        .filter(|e| {
            e.message.contains("axon-T9XX")
                || e.message.contains("axon-T9YY")
                || e.message.contains("axon-W003")
        })
        .collect();
    assert!(
        warns.is_empty(),
        "§Fase 38.x.f §4 — if/else branches AGREEING on Plural + \
         List<T> output is well-formed. No cardinality diagnostic \
         should fire. Errors: {warns:#?}"
    );
}

// ── §5 — D3 bilateral: Singular tail + List<T> output → T9XX ────────

#[test]
fn s5_d3_singular_tail_with_list_output_emits_t9xx() {
    let src = r#"
        type Item { id: Text }
        axonendpoint create_item { public: true
            method: POST
            path: "/api/items"
            output: FlowEnvelope<List<Item>>
            execute: CreateItem
        }
        flow CreateItem() -> Unit {
            step Make { reason: "ok" output: Item }
        }
    "#;
    let errs = check_errors(src);
    let t9xx_bilateral: Vec<&TypeError> = errs.iter()
        .filter(|e| {
            e.message.contains("axon-T9XX")
                && e.message.contains("bilateral_cardinality_mismatch")
        })
        .collect();
    assert!(
        !t9xx_bilateral.is_empty(),
        "§Fase 38.x.f §5 — singular flow tail + `output: List<T>` MUST \
         emit `axon-T9XX` with the bilateral hint. All errors: \
         {errs:#?}"
    );
}

// ── §6 — D5 Stream: retrieve tail + Stream<T> output → T9YY ─────────

#[test]
fn s6_d5_stream_output_with_retrieve_tail_emits_t9yy() {
    let src = r#"
        type Token { text: Text }
        axonstore tokens { backend: in_memory }
        axonendpoint stream_tokens { public: true
            method: POST
            path: "/api/stream"
            output: Stream<Token>
            execute: StreamTokens
            transport: sse
        }
        flow StreamTokens() -> Unit {
            retrieve tokens { where: "1 = 1" as: rows }
        }
    "#;
    let errs = check_errors(src);
    let t9yy: Vec<&TypeError> = errs.iter()
        .filter(|e| e.message.contains("axon-T9YY"))
        .collect();
    assert!(
        !t9yy.is_empty(),
        "§Fase 38.x.f §6 — `output: Stream<T>` + non-stream flow tail \
         (retrieve produces spatial List, not temporal Stream) MUST \
         emit `axon-T9YY stream_cardinality_mismatch`. All errors: \
         {errs:#?}"
    );
}

// ── §7 — D5 Stream: Stream step tail + Stream output → no error ─────

#[test]
fn s7_d5_stream_output_with_stream_step_passes() {
    let src = r#"
        type Token { text: Text }
        axonendpoint stream_chat { public: true
            method: POST
            path: "/api/chat-stream"
            output: Stream<Token>
            execute: StreamChat
            transport: sse
        }
        flow StreamChat() -> Unit {
            step Generate { ask: "generate" output: Stream<Token> }
        }
    "#;
    let errs = check_errors(src);
    let t9yy: Vec<&TypeError> = errs.iter()
        .filter(|e| e.message.contains("axon-T9YY"))
        .collect();
    assert!(
        t9yy.is_empty(),
        "§Fase 38.x.f §7 — `output: Stream<T>` + step with `output: \
         Stream<T>` is well-formed. No T9YY should fire. Errors: \
         {t9yy:#?}"
    );
}

// ── §8 — D5 bilateral Stream: T output + Stream tail → T9YY ─────────

#[test]
fn s8_d5_singular_output_with_stream_tail_emits_t9yy() {
    let src = r#"
        type Token { text: Text }
        axonendpoint wrong_chat { public: true
            method: POST
            path: "/api/wrong-chat"
            output: FlowEnvelope<Token>
            execute: WrongChat
        }
        flow WrongChat() -> Unit {
            step Generate { ask: "..." output: Stream<Token> }
        }
    "#;
    let errs = check_errors(src);
    let t9yy: Vec<&TypeError> = errs.iter()
        .filter(|e| e.message.contains("axon-T9YY"))
        .collect();
    assert!(
        !t9yy.is_empty(),
        "§Fase 38.x.f §8 — singular output `T` + Stream<T> tail MUST \
         emit `axon-T9YY` (the bilateral arm of D5). All errors: \
         {errs:#?}"
    );
}

// ── §9 — D6: output: Any accepts disagreed branches ─────────────────

#[test]
fn s9_d6_any_output_accepts_disagreed_branches() {
    let src = r#"
        type Item { id: Text }
        axonstore data { backend: in_memory }
        axonendpoint flexible { public: true
            method: GET
            path: "/api/flex"
            output: Any
            execute: Flex
        }
        flow Flex() -> Unit {
            if mode == "list" {
                retrieve data { where: "1 = 1" as: rows }
            } else {
                step Single { reason: "ok" output: Item }
            }
        }
    "#;
    let errs = check_errors(src);
    let w003: Vec<&TypeError> = errs.iter()
        .filter(|e| e.message.contains("axon-W003"))
        .collect();
    assert!(
        w003.is_empty(),
        "§Fase 38.x.f §9 — `output: Any` is the documented degraded \
         surface that accepts ANY cardinality including disagreed \
         branches. No W003 should fire. Errors: {w003:#?}"
    );
}

// ── §10 — D2 runtime: BodyValidationError exposes cardinality fields ─

#[test]
fn s10_d2_body_validation_error_has_cardinality_surface() {
    // Pin the public-surface shape of `BodyValidationError`: the
    // v1.40.0 (38.x.f D2) additions MUST be present so adopters
    // reaching for the audit_log entry can grep them.
    let src = include_str!("../src/route_schema.rs");
    assert!(
        src.contains("pub expected_cardinality: String"),
        "§Fase 38.x.f §10 (D2) — `BodyValidationError.expected_cardinality: \
         String` declaration MUST be present in route_schema.rs."
    );
    assert!(
        src.contains("pub got_cardinality: String"),
        "§Fase 38.x.f §10 (D2) — `BodyValidationError.got_cardinality: \
         String` MUST be present."
    );
    assert!(
        src.contains("pub got_length: Option<u64>"),
        "§Fase 38.x.f §10 (D2) — `BodyValidationError.got_length: \
         Option<u64>` MUST be present."
    );
    assert!(
        src.contains("pub remediation_url: String"),
        "§Fase 38.x.f §10 (D2) — `BodyValidationError.remediation_url: \
         String` MUST be present (docs URL for canonical remediation)."
    );
}

// ── §11 — D4 OWASP-safe default + verbose opt-in ────────────────────

#[test]
fn s11_d4_verbose_d5_hint_env_var_recognized() {
    // Pin the canonical `AXON_VERBOSE_D5_HINT` env var name + the
    // truthy alphabet. Without this STATIC grep, a future refactor
    // could rename the env var and break the documented opt-in
    // contract.
    let src = include_str!("../src/axon_server.rs");
    assert!(
        src.contains("AXON_VERBOSE_D5_HINT"),
        "§Fase 38.x.f §11 (D4) — `AXON_VERBOSE_D5_HINT` env var MUST be \
         consulted in axon_server.rs's `internal_validation_500` for \
         the OWASP-safe + opt-in verbose hint contract."
    );
    // Truthy alphabet must include the documented values.
    for token in ["\"1\"", "\"true\"", "\"yes\"", "\"on\""] {
        assert!(
            src.contains(token),
            "§Fase 38.x.f §11 (D4) — verbose hint truthy alphabet MUST \
             include `{token}` (1/true/yes/on, case-insensitive). The \
             contract is documented in the plan vivo §D4 + INTEGRATION \
             guides."
        );
    }
}

// ── §12 — §S STATIC grep: surface declarations present ──────────────

#[test]
fn s12_static_grep_cardinality_surface_present() {
    let src = include_str!("../../axon-frontend/src/type_checker.rs");
    assert!(
        src.contains("pub(crate) enum Cardinality"),
        "§Fase 38.x.f §S — `pub(crate) enum Cardinality` MUST be \
         declared in axon-frontend/src/type_checker.rs. This is the \
         load-bearing D1 enum that the gate consumes."
    );
    for variant in [
        "Singular(String)",
        "Plural(String)",
        "StreamCardinality(String)",
        "Unit,",
        "Disagreed,",
        "Unknown,",
    ] {
        assert!(
            src.contains(variant),
            "§Fase 38.x.f §S — `Cardinality::{variant}` MUST be \
             declared. The 6-variant catalog is the closed surface; \
             removing a variant silently regresses the gate's coverage."
        );
    }
    assert!(
        src.contains("pub(crate) fn infer_flow_tail_cardinality"),
        "§Fase 38.x.f §S — `infer_flow_tail_cardinality` propagation \
         pass MUST be declared. This is the load-bearing analysis."
    );
    assert!(
        src.contains("pub(crate) fn declared_cardinality"),
        "§Fase 38.x.f §S — `declared_cardinality` MUST be declared \
         (maps endpoint `output:` strings to `Cardinality` variants)."
    );
    assert!(
        src.contains("fn emit_cardinality_gate"),
        "§Fase 38.x.f §S — `emit_cardinality_gate` MUST be the central \
         emit point for T9XX/T9YY/W003 diagnostics. A future refactor \
         that inlines the emits scatters the discipline."
    );
}
