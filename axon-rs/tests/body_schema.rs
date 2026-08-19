//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.23.0 — Cross-stack drift gate (Rust side) for body schema
//! validation on dynamic axonendpoint routes.
//!
//! D4 + D9 + D11 ratificadas 2026-05-11. Verifies:
//!
//!   1. The Rust `validate_body` produces the SAME structured
//!      `(expected_type, field_path, expected, got)` tuple that the
//!      Python `axon.runtime.route_schema.validate_body` produces
//!      from the same corpus entry. Drift caught at PR-time per D11.
//!   2. D9 backwards-compat: empty `body_type` short-circuits to
//!      `Ok(())`, regardless of body shape.
//!   3. Primitive validation: String/Integer/Float/Boolean/Any honour
//!      the JSON-tag distinction (integer vs number, etc.).
//!   4. Structured types: required field missing → error with dotted
//!      field_path; optional absent / null OK; extra fields silently
//!      accepted (Postel's Law).
//!   5. Generic `List<T>`: element-wise indexed dotted path on
//!      violation (`values[1]`).
//!   6. Built-in range types: RiskScore + ConfidenceScore ∈ [0,1] and
//!      SentimentScore ∈ [-1,1] rejected out-of-bounds with
//!      `fmt_f64`-formatted bounds (drift-safe across stacks).
//!   7. Unknown declared types surface diagnostic instead of silently
//!      passing.
//!   8. End-to-end HTTP test: declared `body: T` on a POST endpoint
//!      produces 400 Bad Request on a malformed body and 200 OK on a
//!      well-formed body, anchoring the **Kivi-style adopter case**
//!      at the wire layer.
//!
//! Pillar trace per D12:
//!   MATHEMATICS — `validate_body` is pure + total over declared types.
//!   LOGIC      — every accepted body matches declared schema; no
//!                 coercion / widening.
//!   PHILOSOPHY — declaration IS the contract; auditors trace failures
//!                 to the exact field via the dotted path.
//!   COMPUTING  — D9 backwards-compat absolute; D11 cross-stack
//!                 byte-identical on the locked-shape tuple.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use axon::axon_server::{build_router, ServerConfig};
use axon::route_schema::{
    builtin_range, fmt_f64, validate_body, BodyValidationError, FieldSchema,
    TypeSchema, BUILTIN_PRIMITIVES,
};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde::Deserialize;
use serde_json::Value;
use tower::ServiceExt;

// ── Corpus shape ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct Corpus {
    description: String,
    d_letter_anchor: String,
    entries: Vec<Entry>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct Entry {
    name: String,
    #[serde(default)]
    description: String,
    type_declarations: Vec<TypeDecl>,
    body_type: String,
    body: Value,
    expected_validation: Option<ExpectedValidation>,
}

#[derive(Deserialize, PartialEq, Debug)]
struct ExpectedValidation {
    expected_type: String,
    field_path: String,
    expected: String,
    got: String,
}

#[derive(Deserialize)]
struct TypeDecl {
    name: String,
    #[serde(default)]
    fields: Vec<FieldDecl>,
}

#[derive(Deserialize)]
struct FieldDecl {
    name: String,
    #[serde(rename = "type")]
    type_name: String,
    #[serde(default)]
    generic_param: String,
    #[serde(default)]
    optional: bool,
}

fn corpus_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests")
        .join("fixtures")
        .join("body_schema")
        .join("corpus.json")
}

fn load_corpus() -> Corpus {
    let raw = fs::read_to_string(corpus_path())
        .expect("v1.23.0 corpus.json must exist at the shared fixture path");
    serde_json::from_str(&raw).expect("corpus.json must parse")
}

fn build_table(decls: &[TypeDecl]) -> HashMap<String, TypeSchema> {
    let mut table = HashMap::new();
    for decl in decls {
        let fields = decl
            .fields
            .iter()
            .map(|f| FieldSchema {
                name: f.name.clone(),
                type_name: f.type_name.clone(),
                generic_param: f.generic_param.clone(),
                optional: f.optional,
            })
            .collect();
        table.insert(
            decl.name.clone(),
            TypeSchema {
                name: decl.name.clone(),
                fields,
                range: None,
            },
        );
    }
    table
}

fn err_to_expected(err: &BodyValidationError) -> ExpectedValidation {
    ExpectedValidation {
        expected_type: err.expected_type.clone(),
        field_path: err.field_path.clone(),
        expected: err.expected.clone(),
        got: err.got.clone(),
    }
}

// ── Corpus integrity ─────────────────────────────────────────────────

#[test]
fn corpus_loads_with_required_shape() {
    let corpus = load_corpus();
    assert!(corpus.d_letter_anchor.starts_with("D4"));
    assert!(!corpus.description.is_empty());
    assert!(
        corpus.entries.len() >= 25,
        "corpus shrank below 25 entries: {}",
        corpus.entries.len()
    );
}

#[test]
fn rust_validation_matches_corpus_for_every_entry() {
    let corpus = load_corpus();
    let mut failures: Vec<String> = Vec::new();
    for entry in &corpus.entries {
        let table = build_table(&entry.type_declarations);
        let result = validate_body(&entry.body, &entry.body_type, &table);
        match (&result, &entry.expected_validation) {
            (Ok(_), None) => {}
            (Ok(_), Some(exp)) => {
                failures.push(format!(
                    "[{}] expected error {:?} but validation succeeded",
                    entry.name, exp
                ));
            }
            (Err(e), None) => {
                failures.push(format!(
                    "[{}] expected success but got error {:?}",
                    entry.name, err_to_expected(e)
                ));
            }
            (Err(e), Some(exp)) => {
                let actual = err_to_expected(e);
                if &actual != exp {
                    failures.push(format!(
                        "[{}] drift:\n  rust actual:     {:?}\n  corpus expected: {:?}",
                        entry.name, actual, exp
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "Rust ↔ corpus drift in {} entries:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ── Canonical D4 + D9 coverage ───────────────────────────────────────

#[test]
fn d9_empty_body_type_passes_any_body() {
    let table = HashMap::new();
    assert!(validate_body(&serde_json::json!({"any": "shape"}), "", &table).is_ok());
    assert!(validate_body(&serde_json::json!([1, 2, 3]), "", &table).is_ok());
    assert!(validate_body(&serde_json::json!("string"), "", &table).is_ok());
    assert!(validate_body(&serde_json::Value::Null, "", &table).is_ok());
}

#[test]
fn primitive_integer_rejects_float_locked_logic() {
    let table = HashMap::new();
    let err = validate_body(&serde_json::json!(3.14), "Integer", &table).unwrap_err();
    assert_eq!(err.expected, "Integer");
    assert_eq!(err.got, "number");
}

#[test]
fn primitive_float_accepts_integer_json() {
    let table = HashMap::new();
    assert!(validate_body(&serde_json::json!(42), "Float", &table).is_ok());
    assert!(validate_body(&serde_json::json!(-1), "Float", &table).is_ok());
    assert!(validate_body(&serde_json::json!(3.14), "Float", &table).is_ok());
}

#[test]
fn boolean_is_not_integer() {
    // Drift-protect: serde_json::Value::Bool must not satisfy Integer.
    let table = HashMap::new();
    let err = validate_body(&serde_json::json!(true), "Integer", &table).unwrap_err();
    assert_eq!(err.got, "boolean");
}

#[test]
fn structured_dotted_path_on_nested_violation() {
    let mut table = HashMap::new();
    table.insert(
        "Person".to_string(),
        TypeSchema {
            name: "Person".to_string(),
            fields: vec![FieldSchema {
                name: "name".to_string(),
                type_name: "String".to_string(),
                generic_param: String::new(),
                optional: false,
            }],
            range: None,
        },
    );
    table.insert(
        "Loan".to_string(),
        TypeSchema {
            name: "Loan".to_string(),
            fields: vec![FieldSchema {
                name: "applicant".to_string(),
                type_name: "Person".to_string(),
                generic_param: String::new(),
                optional: false,
            }],
            range: None,
        },
    );
    let body = serde_json::json!({"applicant": {}});
    let err = validate_body(&body, "Loan", &table).unwrap_err();
    assert_eq!(err.field_path, "applicant.name");
    assert_eq!(err.got, "missing");
    assert_eq!(err.expected_type, "Loan");
}

#[test]
fn list_indexed_violation_uses_bracket_notation() {
    let mut table = HashMap::new();
    table.insert(
        "Tags".to_string(),
        TypeSchema {
            name: "Tags".to_string(),
            fields: vec![FieldSchema {
                name: "values".to_string(),
                type_name: "List".to_string(),
                generic_param: "String".to_string(),
                optional: false,
            }],
            range: None,
        },
    );
    let body = serde_json::json!({"values": ["a", 42, "c"]});
    let err = validate_body(&body, "Tags", &table).unwrap_err();
    assert_eq!(err.field_path, "values[1]");
    assert_eq!(err.got, "integer");
}

#[test]
fn risk_score_in_bounds_accepts_zero_and_one() {
    let table = HashMap::new();
    assert!(validate_body(&serde_json::json!(0.0), "RiskScore", &table).is_ok());
    assert!(validate_body(&serde_json::json!(1.0), "RiskScore", &table).is_ok());
    assert!(validate_body(&serde_json::json!(0.5), "RiskScore", &table).is_ok());
}

#[test]
fn sentiment_score_negative_bound_accepted() {
    let table = HashMap::new();
    assert!(validate_body(&serde_json::json!(-1.0), "SentimentScore", &table).is_ok());
    assert!(validate_body(&serde_json::json!(0.0), "SentimentScore", &table).is_ok());
    let err = validate_body(&serde_json::json!(-1.5), "SentimentScore", &table).unwrap_err();
    assert!(err.expected.contains("SentimentScore"));
}

#[test]
fn builtin_range_table_anchor() {
    assert_eq!(builtin_range("RiskScore"), Some((0.0, 1.0)));
    assert_eq!(builtin_range("ConfidenceScore"), Some((0.0, 1.0)));
    assert_eq!(builtin_range("SentimentScore"), Some((-1.0, 1.0)));
    assert_eq!(builtin_range("NotRanged"), None);
}

#[test]
fn fmt_f64_matches_python_format() {
    // Cross-stack drift anchor — exactly the strings Python produces.
    assert_eq!(fmt_f64(0.0), "0");
    assert_eq!(fmt_f64(1.0), "1");
    assert_eq!(fmt_f64(-1.0), "-1");
    assert_eq!(fmt_f64(100.0), "100");
    assert_eq!(fmt_f64(1.5), "1.5");
    assert_eq!(fmt_f64(-1.5), "-1.5");
}

#[test]
fn builtin_primitives_constant_anchor() {
    // Anchor: this constant is the closed enum both stacks consult.
    assert!(BUILTIN_PRIMITIVES.contains(&"String"));
    assert!(BUILTIN_PRIMITIVES.contains(&"Integer"));
    assert!(BUILTIN_PRIMITIVES.contains(&"Float"));
    assert!(BUILTIN_PRIMITIVES.contains(&"Boolean"));
    assert!(BUILTIN_PRIMITIVES.contains(&"Duration"));
    assert!(BUILTIN_PRIMITIVES.contains(&"Any"));
    assert!(!BUILTIN_PRIMITIVES.contains(&"Number"));
    assert!(!BUILTIN_PRIMITIVES.contains(&"Object"));
}

#[test]
fn unknown_type_surfaces_diagnostic() {
    let table = HashMap::new();
    let err = validate_body(&serde_json::json!({}), "NotDeclared", &table).unwrap_err();
    assert_eq!(err.expected, "NotDeclared");
    assert!(err.hint.contains("NotDeclared"));
}

#[test]
fn body_validation_error_serialises_to_locked_shape() {
    // BodyValidationError must serialize via serde so the HTTP layer
    // can project it into the JSON response body as-is.
    let err = BodyValidationError {
        expected_type: "X".to_string(),
        field_path: "a.b".to_string(),
        expected: "Y".to_string(),
        got: "string".to_string(),
        hint: "h".to_string(),
        expected_cardinality: String::new(),
        got_cardinality: String::new(),
        got_length: None,
        remediation_url: String::new(),
    };
    let s = serde_json::to_value(&err).unwrap();
    assert_eq!(s["expected_type"], "X");
    assert_eq!(s["field_path"], "a.b");
    assert_eq!(s["expected"], "Y");
    assert_eq!(s["got"], "string");
    assert_eq!(s["hint"], "h");
}

// ── End-to-end HTTP — declared body: T enforced at the wire ────────

fn server_cfg() -> ServerConfig {
    ServerConfig {
        host: "127.0.0.1".into(),
        port: 0,
        channel: "memory".into(),
        auth_token: String::new(),
        log_level: "INFO".into(),
        log_format: "json".into(),
        log_file: None,
        database_url: None,
        config_path: None,
        strict_type_driven_transport: false,
        default_backend: None,
        schemas_dir: None,
    }
}

async fn deploy(app: axum::Router, src: &str) {
    let body = serde_json::json!({
        "source": src,
        "source_file": "test.axon",
        "backend": "stub",
    });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_eq!(status, StatusCode::OK, "deploy failed: {json}");
    assert_eq!(
        json.get("success").and_then(|v| v.as_bool()),
        Some(true),
        "deploy success=false: {json}"
    );
}

fn loan_program() -> &'static str {
    // Two type defs + an axonendpoint with body: LoanApplication.
    // The flow itself is the simplest possible flow whose execute
    // doesn't matter for the body-validation gate.
    "type Money { amount: Float currency: String }\n\
     type LoanApplication { amount: Money applicant: String }\n\
     flow ApproveOrDeny() -> String { let result = \"approved\" return result }\n\
     axonendpoint LoanDecision { public: true method: POST path: \"/loan/decision\" \
        body: LoanApplication execute: ApproveOrDeny }"
}

#[tokio::test]
async fn malformed_body_returns_400_with_body_schema_violation() {
    let app = build_router(server_cfg());
    deploy(app.clone(), loan_program()).await;

    // Send a body where `amount` is a raw number instead of Money.
    let bad_body = serde_json::json!({
        "amount": 50000,
        "applicant": "alice"
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/loan/decision")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&bad_body).unwrap()))
                .unwrap(),
        )
        .await
        .expect("request");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(payload["error"], "body_schema_violation");
    assert_eq!(payload["expected_type"], "LoanApplication");
    assert_eq!(payload["field_path"], "amount");
    assert_eq!(payload["expected"], "Money");
    assert_eq!(payload["d_letter"], "D4");
}

#[tokio::test]
async fn empty_body_on_post_with_body_type_returns_400() {
    let app = build_router(server_cfg());
    deploy(app.clone(), loan_program()).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/loan/decision")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(payload["error"], "body_schema_violation");
    assert_eq!(payload["got"], "missing");
}

#[tokio::test]
async fn invalid_json_body_returns_400() {
    let app = build_router(server_cfg());
    deploy(app.clone(), loan_program()).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/loan/decision")
                .header("content-type", "application/json")
                .body(Body::from("not-json"))
                .unwrap(),
        )
        .await
        .expect("request");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(payload["error"], "body_schema_violation");
    assert_eq!(payload["got"], "invalid_json");
}

#[tokio::test]
async fn d9_empty_body_declaration_accepts_free_form_post_body() {
    let app = build_router(server_cfg());
    let src = "flow Ping() -> String { let result = \"pong\" return result }\n\
               axonendpoint PingEndpoint { public: true method: POST path: \"/ping\" execute: Ping }";
    deploy(app.clone(), src).await;

    // Adopter without `body:` declaration can POST any free-form JSON.
    let any_body = serde_json::json!({"any": "shape", "the": ["client", "sends"]});
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ping")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&any_body).unwrap()))
                .unwrap(),
        )
        .await
        .expect("request");
    // D9 — no body validation runs; the dispatch proceeds. We don't
    // assert on the flow output (stub backend semantics) — only that
    // we DON'T get the 400 body_schema_violation.
    assert_ne!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn well_formed_body_passes_validation_gate() {
    let app = build_router(server_cfg());
    deploy(app.clone(), loan_program()).await;

    let good_body = serde_json::json!({
        "amount": {"amount": 50000.0, "currency": "USD"},
        "applicant": "alice"
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/loan/decision")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&good_body).unwrap()))
                .unwrap(),
        )
        .await
        .expect("request");
    // Validation gate passed. The 200 OR 4xx-from-flow-execution
    // distinction is out of scope — what matters here is that we
    // did NOT short-circuit at body_schema_violation.
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_ne!(payload["error"], "body_schema_violation");
}
