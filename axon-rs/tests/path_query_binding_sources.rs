//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.32.0 — Path + Query as Request Binding sources.
//!
//! This anchor pins the v1.38.5 extension of the v1.36.0 Request
//! Binding Contract: the binding source set grows from "body only" to
//! "path + query + body". The contract semantics carry forward —
//! typed, by-name, totality-enforced — but the SOURCE UNIVERSE is
//! now three.
//!
//! Seven -assertions cover the five D-letters end-to-end:
//!
//! section 1 — D1 — path-param extraction from the `path:` string at parse
//!         time populates `AxonEndpointDefinition.path_params`.
//! section 2 — D3 — flow parameter covered ONLY by a path placeholder
//!         (no body field, no query param) satisfies the D2 totality
//!         check — the legacy v1.38.4 "missing body field" error is
//!         GONE for path-bound params.
//! section 3 — D3 — flow parameter covered ONLY by a `query: { … }` block
//!         entry satisfies the D2 totality check.
//! section 4 — D3 — flow parameter covered across all THREE sources
//!         (path + query + body, one param each) satisfies the
//!         totality check end-to-end through the runtime: every
//!         value reaches the flow's interpolation scope.
//! section 5 — D4 — the same parameter name declared in TWO sources
//!         (path AND body) emits the new `axon-T901
//!         parameter_name_clash` compile error.
//! section 6 — D5 — an endpoint with NO path placeholders + NO `query:`
//!         block + body-only flow is byte-identical to v1.38.4
//!         behavior; the v1.38.4 anchor wire reaches the flow.
//! section 7 — Runtime — the binder merges path + query + body
//!         deterministically (D4 invariant: at most one source per
//!         name; merge order is documentation, not semantics); each
//!         source's value reaches the flow in the declared param
//!         order.
//!
//! Plus a STATIC grep -assertion (S) reading the parser + AST source
//! to pin that the new surface field DECLARATIONS exist — guards
//! against a future refactor accidentally dropping `path_params` from
//! `AxonEndpointDefinition` or `extract_path_param_names` from the
//! parser.

use axon::axon_server::{build_router, ServerConfig};
use axon::ast::Declaration;
use axon::lexer::Lexer;
use axon::parser::Parser;
use axon::type_checker::TypeChecker;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

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

fn parse_program(src: &str) -> axon::ast::Program {
    let tokens = Lexer::new(src, "anchor_2.axon")
        .tokenize()
        .expect("v1.32.0 anchor — lex must succeed");
    Parser::new(tokens)
        .parse()
        .expect("v1.32.0 anchor — parse must succeed")
}

fn type_check_errors(src: &str) -> Vec<axon::type_checker::TypeError> {
    let program = parse_program(src);
    TypeChecker::new(&program).check()
}

async fn deploy_ok(app: &axum::Router, src: &str) {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({ "source": src }).to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_eq!(
        status,
        StatusCode::OK,
        "v1.32.0 anchor — deploy must succeed for valid 37.y source: {json}"
    );
    assert_eq!(
        json.get("success").and_then(|v| v.as_bool()),
        Some(true),
        "v1.32.0 anchor — deploy must report success=true: {json}"
    );
}

async fn hit_sse(app: &axum::Router, method: &str, path: &str, body: &str) -> String {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "v1.32.0 anchor — {method} {path} must return 200"
    );
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).into_owned()
}

const ECHO_TOOL: &str = "tool Echo { provider: stub_stream description: \"echo\" \
                         effects: <stream:drop_oldest> }\n";

// ── section 1 — D1 path-param extraction at parse time ─────────────────────

#[test]
fn s1_d1_path_param_extraction_at_parse_time() {
    // The kivi corpus path. The parser scans for `{name}` placeholders
    // and records them on `AxonEndpointDefinition.path_params` in
    // declaration order.
    let src = format!(
        "type SecretWriteRequest {{ value: String }}\n\
         {ECHO_TOOL}\
         flow WriteSecret(tenant_id: Text, secret_name: Text, value: String) -> Unit {{\n\
             step S {{ ask: \"t=${{tenant_id}}|n=${{secret_name}}|v=${{value}}\" apply: Echo }}\n\
         }}\n\
         axonendpoint WriteSecretE {{ public: true method: POST \
             path: \"/api/tenants/{{tenant_id}}/secrets/{{secret_name}}\" \
             body: SecretWriteRequest execute: WriteSecret \
             backend: stub transport: sse }}"
    );

    let program = parse_program(&src);
    let endpoint = program
        .declarations
        .iter()
        .find_map(|d| match d {
            Declaration::AxonEndpoint(e) if e.name == "WriteSecretE" => Some(e),
            _ => None,
        })
        .expect("v1.32.0 section 1 — the `WriteSecretE` endpoint must parse");

    assert_eq!(
        endpoint.path_params,
        vec!["tenant_id".to_string(), "secret_name".to_string()],
        "v1.32.0 D1 — `path: \"/api/tenants/{{tenant_id}}/secrets/{{secret_name}}\"` \
         must yield `path_params = [\"tenant_id\", \"secret_name\"]` (declaration order). \
         Actual: {:?}",
        endpoint.path_params
    );
}

// ── section 2 — D3 path-only param coverage satisfies totality ─────────────

#[test]
fn s2_d3_path_only_param_coverage_satisfies_totality() {
    // The endpoint's body type carries `value` only; `tenant_id` is
    // satisfied SOLELY by the path placeholder. Pre-37.y this was the
    // exact compile error the kivi adopter reported. Post-37.y the D3
    // union check sees `tenant_id` covered by `path_params` and
    // accepts the program.
    let src = format!(
        "type SecretBody {{ value: String }}\n\
         {ECHO_TOOL}\
         flow WriteSecret(tenant_id: Text, value: String) -> Unit {{\n\
             step S {{ ask: \"t=${{tenant_id}}|v=${{value}}\" apply: Echo }}\n\
         }}\n\
         axonendpoint WriteSecretE {{ public: true method: POST \
             path: \"/api/tenants/{{tenant_id}}/secrets\" \
             body: SecretBody execute: WriteSecret \
             backend: stub transport: sse }}"
    );

    let errors = type_check_errors(&src);
    // No Request Binding error in the result set.
    let binding_errors: Vec<&axon::type_checker::TypeError> = errors
        .iter()
        .filter(|e| {
            e.message.contains("Request Binding")
                || e.message.contains("axon-T901")
                || e.message.contains("no matching field in body")
        })
        .collect();
    assert!(
        binding_errors.is_empty(),
        "v1.32.0 D3 — a flow param covered ONLY by a path placeholder must \
         satisfy the D2 totality check; the legacy v1.38.4 \"no matching \
         field in body\" error is gone. Errors: {binding_errors:#?}"
    );
}

// ── section 3 — D3 query-only param coverage satisfies totality ────────────

#[test]
fn s3_d3_query_only_param_coverage_satisfies_totality() {
    // No `body:` declared; the flow's `status: Text?` param is
    // satisfied SOLELY by the `query: { status: Text? }` entry.
    // (Optional `?` round-trips through `parse_type_expr` identically
    // for query params + flow params, so the totality check on the
    // optional flag is uniform.)
    let src = format!(
        "{ECHO_TOOL}\
         flow ListSecrets(status: Text) -> Unit {{\n\
             step S {{ ask: \"s=${{status}}\" apply: Echo }}\n\
         }}\n\
         axonendpoint ListSecretsE {{ public: true method: GET \
             path: \"/api/secrets\" \
             query: {{ status: Text }} \
             execute: ListSecrets \
             backend: stub transport: sse }}"
    );

    let errors = type_check_errors(&src);
    let binding_errors: Vec<&axon::type_checker::TypeError> = errors
        .iter()
        .filter(|e| {
            e.message.contains("Request Binding")
                || e.message.contains("axon-T901")
                || e.message.contains("no matching field in body")
        })
        .collect();
    assert!(
        binding_errors.is_empty(),
        "v1.32.0 D3 — a flow param covered ONLY by a `query: {{ … }}` entry \
         must satisfy the D2 totality check (optional flag included). \
         Errors: {binding_errors:#?}"
    );
}

// ── section 4 — D3 mixed coverage (path + query + body) end-to-end ─────────

#[tokio::test]
async fn s4_d3_mixed_coverage_path_query_body_end_to_end() {
    let app = build_router(server_cfg());
    // The kivi-style combined endpoint: tenant_id from path, dry_run
    // from query, value from body. All three threaded into the same
    // step's `ask:` so the runtime delivery is directly observable on
    // the wire.
    let src = format!(
        "type WriteBody {{ value: String }}\n\
         {ECHO_TOOL}\
         flow WriteSecret(tenant_id: Text, dry_run: Text, value: String) -> Unit {{\n\
             step S {{ ask: \"t=${{tenant_id}}|d=${{dry_run}}|v=${{value}}\" apply: Echo }}\n\
         }}\n\
         axonendpoint WriteSecretE {{ public: true method: POST \
             path: \"/api/tenants/{{tenant_id}}/secrets\" \
             query: {{ dry_run: Text }} \
             body: WriteBody execute: WriteSecret \
             backend: stub transport: sse }}"
    );
    deploy_ok(&app, &src).await;

    let wire = hit_sse(
        &app,
        "POST",
        "/api/tenants/TENANT_S4/secrets?dry_run=DRY_S4",
        r#"{"value":"VALUE_S4"}"#,
    )
    .await;

    assert!(
        wire.contains("t=TENANT_S4"),
        "v1.32.0 D3 — path placeholder `{{tenant_id}}` must capture the \
         URL segment and bind to the same-named flow param. Wire:\n{wire}"
    );
    assert!(
        wire.contains("d=DRY_S4"),
        "v1.32.0 D3 — query param `dry_run` must capture the URL query \
         entry and bind to the same-named flow param. Wire:\n{wire}"
    );
    assert!(
        wire.contains("v=VALUE_S4"),
        "v1.32.0 D3 — body field `value` must bind to the same-named flow \
         param (v1.36.0 surface, unchanged). Wire:\n{wire}"
    );
    assert!(
        !wire.contains("${tenant_id}")
            && !wire.contains("${dry_run}")
            && !wire.contains("${value}"),
        "v1.32.0 D3 — every flow param interpolates from its source; no \
         `${{name}}` token may survive un-interpolated. Wire:\n{wire}"
    );
}

// ── section 5 — D4 collision T901 ──────────────────────────────────────────

#[test]
fn s5_d4_path_and_body_collision_emits_axon_t901() {
    // `tenant_id` declared in BOTH the path AND the body type. The D4
    // strict-disambiguation rule rejects this at compile time so the
    // runtime never has to pick a precedence.
    let src = format!(
        "type ClashBody {{ tenant_id: Text value: String }}\n\
         {ECHO_TOOL}\
         flow WriteSecret(tenant_id: Text, value: String) -> Unit {{\n\
             step S {{ ask: \"t=${{tenant_id}}|v=${{value}}\" apply: Echo }}\n\
         }}\n\
         axonendpoint WriteSecretE {{ public: true method: POST \
             path: \"/api/tenants/{{tenant_id}}/secrets\" \
             body: ClashBody execute: WriteSecret \
             backend: stub transport: sse }}"
    );

    let errors = type_check_errors(&src);
    let t901 = errors.iter().find(|e| e.message.contains("axon-T901"));
    let t901 = t901.expect(&format!(
        "v1.32.0 D4 — same param name in path AND body must emit \
         `axon-T901 parameter_name_clash` at compile time. Errors: \
         {errors:#?}"
    ));
    assert!(
        t901.message.contains("tenant_id"),
        "v1.32.0 D4 — T901 must name the colliding parameter (`tenant_id`). \
         Message: {}",
        t901.message
    );
    assert!(
        t901.message.contains("path") && t901.message.contains("body"),
        "v1.32.0 D4 — T901 must name BOTH colliding sources (\"path\" + \
         \"body\"). Message: {}",
        t901.message
    );
}

// ── section 6 — D5 backwards-compat (body-only path is byte-identical) ─────

#[tokio::test]
async fn s6_d5_body_only_backwards_compat_is_intact() {
    let app = build_router(server_cfg());
    // The v1.38.4 anchor surface: no `{name}` in path, no `query:`
    // block, body-only flow. The D5 contract pins this to byte-
    // identical behavior — `bind_request` collapses to the v1.36.0
    // `bind_request_body` delegate with empty path + empty query maps.
    let src = format!(
        "type LegacyBody {{ payload: String }}\n\
         {ECHO_TOOL}\
         flow LegacyFlow(payload: String) -> Unit {{\n\
             step S {{ ask: \"p=${{payload}}\" apply: Echo }}\n\
         }}\n\
         axonendpoint LegacyE {{ public: true method: POST path: \"/legacy\" \
             body: LegacyBody execute: LegacyFlow \
             backend: stub transport: sse }}"
    );
    deploy_ok(&app, &src).await;

    let wire = hit_sse(&app, "POST", "/legacy", r#"{"payload":"LEGACY_S6"}"#).await;

    assert!(
        wire.contains("p=LEGACY_S6"),
        "v1.32.0 D5 — an endpoint with no path placeholders + no `query:` \
         block must bind body-only EXACTLY as v1.38.4 (byte-identical \
         contract). Wire:\n{wire}"
    );
    assert!(
        !wire.contains("${payload}"),
        "v1.32.0 D5 — the v1.38.4 interpolation behavior is preserved \
         verbatim. Wire:\n{wire}"
    );
}

// ── section 7 — Runtime merges path + query + body in declared param order ──

#[tokio::test]
async fn s7_runtime_binder_merges_sources_in_declaration_order() {
    let app = build_router(server_cfg());
    // Five params spanning the three sources. The flow's parameter
    // declaration order is path1, path2, query1, query2, body1; the
    // step `ask:` interpolates them in a fixed order, so a successful
    // wire pins (a) every source delivers its value and (b) D4
    // invariant: no value is ever "overridden" by another source
    // because no name appears in two sources.
    let src = format!(
        "type WriteBody {{ value: String }}\n\
         {ECHO_TOOL}\
         flow WriteSecret(\
             tenant_id: Text, secret_name: Text, \
             dry_run: Text, overwrite: Text, \
             value: String\
         ) -> Unit {{\n\
             step S {{ ask: \"t=${{tenant_id}}|s=${{secret_name}}|d=${{dry_run}}|o=${{overwrite}}|v=${{value}}\" apply: Echo }}\n\
         }}\n\
         axonendpoint WriteSecretE {{ public: true method: POST \
             path: \"/api/tenants/{{tenant_id}}/secrets/{{secret_name}}\" \
             query: {{ dry_run: Text, overwrite: Text }} \
             body: WriteBody execute: WriteSecret \
             backend: stub transport: sse }}"
    );
    deploy_ok(&app, &src).await;

    let wire = hit_sse(
        &app,
        "POST",
        "/api/tenants/TEN_S7/secrets/SEC_S7?dry_run=DRY_S7&overwrite=OVR_S7",
        r#"{"value":"VAL_S7"}"#,
    )
    .await;

    // Each of the 5 values from its dedicated source must arrive in
    // the step's interpolation scope.
    assert!(
        wire.contains("t=TEN_S7"),
        "v1.32.0 section 7 — path[tenant_id] must deliver. Wire:\n{wire}"
    );
    assert!(
        wire.contains("s=SEC_S7"),
        "v1.32.0 section 7 — path[secret_name] must deliver. Wire:\n{wire}"
    );
    assert!(
        wire.contains("d=DRY_S7"),
        "v1.32.0 section 7 — query[dry_run] must deliver. Wire:\n{wire}"
    );
    assert!(
        wire.contains("o=OVR_S7"),
        "v1.32.0 section 7 — query[overwrite] must deliver. Wire:\n{wire}"
    );
    assert!(
        wire.contains("v=VAL_S7"),
        "v1.32.0 section 7 — body[value] must deliver. Wire:\n{wire}"
    );
    // D4 invariant — no leftover unsubstituted tokens.
    for token in [
        "${tenant_id}",
        "${secret_name}",
        "${dry_run}",
        "${overwrite}",
        "${value}",
    ] {
        assert!(
            !wire.contains(token),
            "v1.32.0 section 7 — no `{token}` token may survive un-interpolated; \
             the binder MUST deliver every flow param from its \
             D4-guaranteed unique source. Wire:\n{wire}"
        );
    }
}

// ── S — STATIC grep: surface fields + parser hook are present ──────

#[test]
fn s_static_grep_surface_fields_and_parser_hook_are_present() {
    // Read the AST + parser sources from this repository at test time
    // and pin the 37.y surface declarations. This is a regression
    // guard: a future refactor that accidentally drops
    // `path_params: Vec<String>` from `AxonEndpointDefinition` (or
    // removes the `extract_path_param_names` hook from the parser)
    // will fail this assertion before the runtime tests above even
    // load the module.
    let ast_src = include_str!("../../axon-frontend/src/ast.rs");
    assert!(
        ast_src.contains("pub path_params: Vec<String>"),
        "v1.32.0 S — `AxonEndpointDefinition.path_params: Vec<String>` \
         declaration must exist verbatim in `axon-frontend/src/ast.rs`. \
         This field is the load-bearing D1 surface; removing it is the \
         most common way a future refactor would silently regress 37.y."
    );
    assert!(
        ast_src.contains("pub query_params: Vec<TypeField>"),
        "v1.32.0 S — `AxonEndpointDefinition.query_params: Vec<TypeField>` \
         declaration must exist verbatim in `axon-frontend/src/ast.rs`. \
         This field is the load-bearing D2 surface."
    );

    let parser_src = include_str!("../../axon-frontend/src/parser.rs");
    assert!(
        parser_src.contains("pub(crate) fn extract_path_param_names("),
        "v1.32.0 S — `extract_path_param_names` helper must exist in \
         `axon-frontend/src/parser.rs`. The parser's `parse_axonendpoint` \
         calls it after the `path:` field is consumed; dropping it \
         silently regresses D1 to an empty `path_params` vec."
    );
    assert!(
        parser_src.contains("pub const AXONENDPOINT_QUERY_PARAM_TYPES"),
        "v1.32.0 S — closed query-param type catalog \
         `AXONENDPOINT_QUERY_PARAM_TYPES` must exist in \
         `axon-frontend/src/parser.rs`. This is the D2 catalog \
         enforcing the {{Text, Int, Float, Bool, Uuid}} closure at \
         parse time."
    );

    let binder_src = include_str!("../src/request_binding.rs");
    assert!(
        binder_src.contains("pub fn bind_request("),
        "v1.32.0 S — the new `bind_request(flow, path, query, body)` \
         signature must exist in `axon-rs/src/request_binding.rs`. \
         This is the 3-source merging point at runtime."
    );
    assert!(
        binder_src.contains("pub fn bind_request_body("),
        "v1.32.0 S — the legacy `bind_request_body(flow, body)` delegate \
         must exist in `axon-rs/src/request_binding.rs` for D5 source \
         backwards-compat with v1.36.0 callers."
    );

    let server_src = include_str!("../src/axon_server.rs");
    assert!(
        // v2.35.0 (Kivi brief #54) promoted this from `pub(crate)` to
        // `pub` so the enterprise `flow_dispatch::DispatchTable` reuses
        // this matcher instead of forking a parallel implementation.
        server_src.contains("pub fn match_path_template("),
        "v1.32.0 S — `match_path_template` helper must exist in \
         `axon-rs/src/axon_server.rs`. This is the template-matching \
         scanner backing the dynamic-route dispatcher's two-step \
         lookup (fast exact + template-match fallback)."
    );
    assert!(
        server_src.contains("pub path_params: Vec<String>"),
        "v1.32.0 S — `DynamicEndpointRoute.path_params: Vec<String>` \
         declaration must exist in `axon-rs/src/axon_server.rs`. \
         The dispatcher gates the template-match fallback on this \
         field being non-empty."
    );
}
