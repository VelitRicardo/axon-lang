//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.23.0 — Cross-stack drift gate (Rust side) for axonendpoint
//! route collection + dynamic route runtime.
//!
//! D1, D2, D3, D11 ratificadas 2026-05-11. Verifies:
//!
//!   1. The Rust `collect_axonendpoint_routes` produces the same
//!      `(method, path) → DynamicEndpointRoute` map that the Python
//!      `axon.runtime.route_registry.collect_axonendpoint_routes`
//!      produces from the same source. Drift caught at PR-time per
//!      D11.
//!   2. Closed method enum (D3): parser rejects HEAD/OPTIONS/etc.
//!   3. Path collision detection (D2): intra-program + cross-deploy.
//!   4. `merge_dynamic_routes` honors same-endpoint re-deploy +
//!      rejects different-endpoint collisions atomically.
//!   5. Runtime fallback handler dispatches `POST /chat` (declared
//! path) through v1.21.0/31 negotiation classifier correctly.
//!   6. 404 with structured `axonendpoint_not_found` for unknown
//!      paths after deploy.
//!
//! Pillar trace per D12:
//!   MATHEMATICS — function is pure + total + cross-stack
//!                  byte-identical.
//!   LOGIC      — collision detection is exhaustive.
//!   PHILOSOPHY — declarative source IS the HTTP behavior.
//!   COMPUTING  — D8+D9 absolute backwards-compat: /v1/execute
//!                 preserved verbatim alongside dynamic routes.

use std::fs;
use std::path::PathBuf;

use axon::axon_server::{
    build_router, collect_axonendpoint_routes, merge_dynamic_routes,
    DynamicEndpointRoute, ServerConfig, AXONENDPOINT_METHODS,
};
use axon::type_checker::compute_implicit_transports;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde::Deserialize;
use tower::ServiceExt;

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct ExpectedRoute {
    method: String,
    path: String,
    flow_name: String,
    endpoint_name: String,
    #[serde(default)]
    transport: String,
    #[serde(default)]
    transport_explicit: bool,
    #[serde(default)]
    keepalive: String,
    #[serde(default)]
    implicit_transport: String,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct CorpusEntry {
    name: String,
    source: String,
    source_file: String,
    expected_parse_ok: bool,
    #[serde(default)]
    expected_error_contains: String,
    #[serde(default)]
    expected_collect_error_contains: String,
    #[serde(default)]
    expected_routes: Vec<ExpectedRoute>,
}

fn corpus_path() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent()
        .expect("CARGO_MANIFEST_DIR has parent")
        .join("tests")
        .join("fixtures")
        .join("routes")
        .join("corpus.json")
}

fn load_corpus() -> Vec<CorpusEntry> {
    let path = corpus_path();
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("failed to parse corpus.json: {e}"))
}

fn project_routes_sorted(
    routes: &std::collections::HashMap<(String, String), DynamicEndpointRoute>,
) -> Vec<(String, String, String, String, String, bool, String, String)> {
    let mut keys: Vec<_> = routes.keys().cloned().collect();
    keys.sort();
    keys.into_iter()
        .map(|k| {
            let r = &routes[&k];
            (
                k.0.clone(),
                k.1.clone(),
                r.flow_name.clone(),
                r.endpoint_name.clone(),
                r.transport.clone(),
                r.transport_explicit,
                r.keepalive.clone(),
                r.implicit_transport.clone(),
            )
        })
        .collect()
}

fn project_expected(
    expected: &[ExpectedRoute],
) -> Vec<(String, String, String, String, String, bool, String, String)> {
    expected
        .iter()
        .map(|r| {
            (
                r.method.clone(),
                r.path.clone(),
                r.flow_name.clone(),
                r.endpoint_name.clone(),
                r.transport.clone(),
                r.transport_explicit,
                r.keepalive.clone(),
                r.implicit_transport.clone(),
            )
        })
        .collect()
}

// ─── section 1 — Corpus integrity ──────────────────────────────────────────

#[test]
fn corpus_loads_with_required_shape() {
    let corpus = load_corpus();
    assert!(corpus.len() >= 12, "corpus must have ≥ 12 entries");
    let mut seen: std::collections::HashSet<&str> =
        std::collections::HashSet::new();
    for entry in &corpus {
        assert!(seen.insert(&entry.name), "duplicate entry: {}", entry.name);
    }
}

// ─── section 2 — Cross-stack route table parity (D11) ──────────────────────

#[test]
fn rust_route_table_matches_corpus() {
    let corpus = load_corpus();
    let mut asserted = 0usize;
    for entry in &corpus {
        // Negative parse cases — Rust parser must reject too.
        if !entry.expected_parse_ok {
            let parse_err = match axon::lexer::Lexer::new(&entry.source, &entry.source_file)
                .tokenize()
            {
                Err(e) => format!("{e:?}"),
                Ok(toks) => match axon::parser::Parser::new(toks).parse() {
                    Err(e) => format!("{e:?}"),
                    Ok(_) => panic!(
                        "entry {}: expected parse to fail but it succeeded",
                        entry.name
                    ),
                },
            };
            if !entry.expected_error_contains.is_empty() {
                assert!(
                    parse_err.contains(&entry.expected_error_contains),
                    "entry {}: expected error to contain '{}', got: {}",
                    entry.name, entry.expected_error_contains, parse_err
                );
            }
            asserted += 1;
            continue;
        }

        // Positive parse path.
        let tokens = axon::lexer::Lexer::new(&entry.source, &entry.source_file)
            .tokenize()
            .unwrap_or_else(|e| panic!("entry {}: lex failed: {e:?}", entry.name));
        let mut program = axon::parser::Parser::new(tokens)
            .parse()
            .unwrap_or_else(|e| panic!("entry {}: parse failed: {e:?}", entry.name));
        compute_implicit_transports(&mut program);

        let routes_result = collect_axonendpoint_routes(
            &program, &entry.source, &entry.source_file,
        );

        if !entry.expected_collect_error_contains.is_empty() {
            // Expected to fail at collect (intra-program collision D2).
            let err = routes_result.err().unwrap_or_else(|| {
                panic!(
                    "entry {}: expected collect error containing '{}' but collect succeeded",
                    entry.name, entry.expected_collect_error_contains
                )
            });
            assert!(
                err.contains(&entry.expected_collect_error_contains),
                "entry {}: expected '{}' in error, got: {}",
                entry.name, entry.expected_collect_error_contains, err
            );
            asserted += 1;
            continue;
        }

        // Normal: assert route table.
        let routes = routes_result.unwrap_or_else(|e| {
            panic!("entry {}: collect failed unexpectedly: {e}", entry.name)
        });
        let actual = project_routes_sorted(&routes);
        let expected = project_expected(&entry.expected_routes);
        assert_eq!(
            actual, expected,
            "entry {}: cross-stack drift\n  expected: {expected:?}\n  actual:   {actual:?}",
            entry.name
        );
        asserted += 1;
    }
    assert!(
        asserted >= 12,
        "expected ≥ 12 corpus entries asserted, got {asserted}"
    );
}

// ─── section 3 — Method enum (D3) ──────────────────────────────────────────

#[test]
fn method_enum_constant_matches_parser_set() {
    // D3 contract anchor: the runtime constant and the parser's
    // closed set must match exactly. Drift here would silently
    // allow methods at parse but reject at runtime (or vice versa).
    let runtime_methods: std::collections::HashSet<&&str> =
        AXONENDPOINT_METHODS.iter().collect();
    let parser_methods: std::collections::HashSet<&&str> =
        axon_frontend::parser::AXONENDPOINT_METHOD_VALUES
            .iter()
            .collect();
    assert_eq!(runtime_methods, parser_methods);
}

// ─── section 4 — D2 cross-deploy collision via merge_dynamic_routes ────────

#[test]
fn cross_deploy_collision_different_endpoint_rejected() {
    let src1 = "flow F() -> Unit { step S { ask: \"x\" } }\n\
                axonendpoint Alpha { public: true method: POST path: \"/chat\" execute: F }";
    let src2 = "flow G() -> Unit { step T { ask: \"y\" } }\n\
                axonendpoint Beta { public: true method: POST path: \"/chat\" execute: G }";

    let collect = |src: &str, file: &str| {
        let tokens = axon::lexer::Lexer::new(src, file).tokenize().unwrap();
        let mut p = axon::parser::Parser::new(tokens).parse().unwrap();
        compute_implicit_transports(&mut p);
        collect_axonendpoint_routes(&p, src, file).unwrap()
    };

    let mut live = std::collections::HashMap::new();
    merge_dynamic_routes(&mut live, collect(src1, "a.axon")).unwrap();
    assert_eq!(live.len(), 1);

    let err = merge_dynamic_routes(&mut live, collect(src2, "b.axon")).err().unwrap();
    assert!(err.contains("cross-deploy"));
    assert!(err.contains("Alpha"));
    assert!(err.contains("Beta"));
}

#[test]
fn same_endpoint_redeploy_updates_in_place() {
    let src1 = "flow F() -> Unit { step S { ask: \"x\" } }\n\
                axonendpoint E { public: true method: POST path: \"/chat\" execute: F }";
    let src2 = "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
                flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
                axonendpoint E { public: true method: POST path: \"/chat\" execute: F transport: sse }";

    let collect = |src: &str, file: &str| {
        let tokens = axon::lexer::Lexer::new(src, file).tokenize().unwrap();
        let mut p = axon::parser::Parser::new(tokens).parse().unwrap();
        compute_implicit_transports(&mut p);
        collect_axonendpoint_routes(&p, src, file).unwrap()
    };

    let mut live = std::collections::HashMap::new();
    merge_dynamic_routes(&mut live, collect(src1, "a.axon")).unwrap();
    assert!(!live[&("POST".to_string(), "/chat".to_string())].transport_explicit);

    merge_dynamic_routes(&mut live, collect(src2, "b.axon")).unwrap();
    let entry = &live[&("POST".to_string(), "/chat".to_string())];
    assert!(entry.transport_explicit);
    assert_eq!(entry.transport, "sse");
}

#[test]
fn merge_is_atomic_on_collision_failure() {
    let src_pre = "flow F() -> Unit { step S { ask: \"x\" } }\n\
                   axonendpoint Existing { public: true method: POST path: \"/chat\" execute: F }";
    let src_new = "flow G() -> Unit { step T { ask: \"y\" } }\n\
                   axonendpoint Fresh    { public: true method: POST path: \"/new\" execute: G }\n\
                   axonendpoint Conflict { public: true method: POST path: \"/chat\" execute: G }";

    let collect = |src: &str, file: &str| {
        let tokens = axon::lexer::Lexer::new(src, file).tokenize().unwrap();
        let mut p = axon::parser::Parser::new(tokens).parse().unwrap();
        compute_implicit_transports(&mut p);
        collect_axonendpoint_routes(&p, src, file).unwrap()
    };

    let mut live = std::collections::HashMap::new();
    merge_dynamic_routes(&mut live, collect(src_pre, "a.axon")).unwrap();
    let incoming = collect(src_new, "b.axon");
    let err = merge_dynamic_routes(&mut live, incoming).err().unwrap();
    assert!(err.contains("Conflict"));

    // /new must NOT be in live — atomic rollback.
    assert!(!live.contains_key(&("POST".to_string(), "/new".to_string())));
    // /chat must still be Existing's.
    let chat = &live[&("POST".to_string(), "/chat".to_string())];
    assert_eq!(chat.endpoint_name, "Existing");
}

// ─── section 5 — Runtime end-to-end: declared path serves the flow ─────────

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

#[tokio::test]
async fn declared_path_serves_the_flow_kivi_case() {
    // THE Kivi case 2026-05-11 — POST /chat directly serves the
    // declared flow without going through /v1/execute. This is the
    // architectural unlock v1.23.0 ships.
    let app = build_router(server_cfg());
    let src = "tool chat_token_stream { description: \"stream\" effects: <stream:drop_oldest> }\n\
               flow Chat() -> Unit { step Generate { ask: \"hi\" apply: chat_token_stream } }\n\
               axonendpoint ChatEndpoint { public: true method: POST path: \"/chat\" execute: Chat transport: sse }";
    deploy(app.clone(), src).await;

    let req = Request::builder()
        .method("POST")
        .uri("/chat")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    // Declared transport: sse → SSE response on the declared path.
    assert!(
        ct.starts_with("text/event-stream"),
        "declared `transport: sse` on `POST /chat` should produce SSE, got {ct}"
    );
}

#[tokio::test]
async fn unknown_path_returns_404_with_registered_routes_list() {
    let app = build_router(server_cfg());
    let src = "flow F() -> Unit { step S { ask: \"x\" } }\n\
               axonendpoint E { public: true method: POST path: \"/chat\" execute: F }";
    deploy(app.clone(), src).await;

    let req = Request::builder()
        .method("POST")
        .uri("/nonexistent")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["error"], "axonendpoint_not_found");
    assert_eq!(json["method"], "POST");
    assert_eq!(json["path"], "/nonexistent");
    assert!(json["registered_routes"].is_array());
    let routes = json["registered_routes"].as_array().unwrap();
    assert!(routes.iter().any(|r| r["path"] == "/chat"));
}

#[tokio::test]
async fn v1_execute_legacy_preserved_alongside_dynamic_routes_d10() {
    // D10 absolute backwards-compat: /v1/execute continues to work
    // verbatim even when dynamic routes are registered for the same
    // flow at a different path.
    let app = build_router(server_cfg());
    let src = "flow F() -> Unit { step S { ask: \"x\" } }\n\
               axonendpoint E { public: true method: POST path: \"/chat\" execute: F }";
    deploy(app.clone(), src).await;

    // Legacy /v1/execute path with body `{"flow": "F"}` must still
    // work — v1.21.0/31 contracts unchanged.
    let body = serde_json::json!({"flow": "F", "backend": "stub"});
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn deploy_with_path_collision_returns_409_style_error_at_deploy() {
    // Intra-program collision (D2) — deploy fails BEFORE routes are
    // registered, audit trail not polluted.
    let app = build_router(server_cfg());
    let src = "flow F() -> Unit { step S { ask: \"x\" } }\n\
               flow G() -> Unit { step T { ask: \"y\" } }\n\
               axonendpoint Alpha { public: true method: POST path: \"/chat\" execute: F }\n\
               axonendpoint Beta  { public: true method: POST path: \"/chat\" execute: G }";
    let body = serde_json::json!({
        "source": src,
        "source_file": "collide.axon",
        "backend": "stub",
    });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK); // deploy_handler returns 200 with success:false
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["success"], false);
    assert_eq!(json["phase"], "route_registration");
    assert_eq!(json["d_letter"], "D2");
    assert!(json["error"].as_str().unwrap().contains("Path collision"));
}

#[tokio::test]
async fn five_method_enum_all_register_correctly() {
    // D3 — every adopter-declarable method produces a valid route.
    let app = build_router(server_cfg());
    let src = "flow F() -> Unit { step S { ask: \"x\" } }\n\
               axonendpoint G  { public: true method: GET    path: \"/g\"  execute: F }\n\
               axonendpoint P  { public: true method: POST   path: \"/p\"  execute: F }\n\
               axonendpoint Pu { public: true method: PUT    path: \"/pu\" execute: F }\n\
               axonendpoint De { public: true method: DELETE path: \"/d\"  execute: F }\n\
               axonendpoint Pa { public: true method: PATCH  path: \"/pa\" execute: F }";
    deploy(app.clone(), src).await;

    // Each of the five methods should route to the flow (200) rather
    // than fall through to 404.
    for (method, path) in [
        ("GET", "/g"),
        ("POST", "/p"),
        ("PUT", "/pu"),
        ("DELETE", "/d"),
        ("PATCH", "/pa"),
    ] {
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "method {method} path {path} should route to flow, got {}",
            resp.status()
        );
    }
}
