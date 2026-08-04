//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 109.b — the proof-carrying derivative, END TO END: deploy a
//! program whose flow binds an expression, `grad`s it, and returns the
//! evaluated gradient — through the REAL `/v1/deploy` + `/v1/execute`
//! handlers (the §95.f discipline). The number that comes back was
//! DERIVED at compile time and EVALUATED at runtime — no model, no
//! tape, no finite differences.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::util::ServiceExt;

fn server_cfg() -> axon::axon_server::ServerConfig {
    axon::axon_server::ServerConfig {
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

#[tokio::test]
async fn deployed_grad_evaluates_the_compile_time_derivative() {
    let (app, _state) = axon::axon_server::build_router_with_state(server_cfg());
    // total = 3x + y·y  ⇒  ∂/∂x = 3 ;  ∂/∂y = y + y.
    // At x=2, y=5:  g = {x: 3, y: 10}.
    let src = r#"
flow Score(x: Float, y: Float) -> Text {
    let total = 3.0 * x + y * y
    grad total wrt [x, y] as g
    return g
}
"#;
    let deploy_body = serde_json::json!({
        "source": src, "source_file": "grad.axon", "backend": "stub",
    });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(deploy_body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let dep: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_eq!(dep["success"], true, "{dep}");

    let exec_body = serde_json::json!({
        "flow": "Score", "backend": "stub",
        "request_body": { "x": 2.0, "y": 5.0 },
    });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute")
        .header("content-type", "application/json")
        .body(Body::from(exec_body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let out: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    let result = &out["result"];
    let g: serde_json::Value = match result {
        serde_json::Value::String(sv) => serde_json::from_str(sv)
            .unwrap_or_else(|_| panic!("gradient JSON expected, got: {out}")),
        v => v.clone(),
    };
    assert_eq!(g["x"], 3.0, "∂(3x + y²)/∂x = 3 — DERIVED, then evaluated: {out}");
    assert_eq!(g["y"], 10.0, "∂/∂y at y=5 = 2y = 10: {out}");
}

#[tokio::test]
async fn t931_refuses_at_deploy_through_the_real_gate() {
    // The compile gate runs inside /v1/deploy: a non-differentiable grad
    // never reaches the runtime.
    let (app, _state) = axon::axon_server::build_router_with_state(server_cfg());
    let src = r#"
flow F(x: Float) -> Text {
    let e = x % 2
    grad e wrt x
}
"#;
    let body = serde_json::json!({
        "source": src, "source_file": "bad.axon", "backend": "stub",
    });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let dep: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_eq!(dep["success"], false, "{dep}");
    assert!(
        dep["error"].as_str().unwrap_or_default().contains("T931"),
        "the refusal names the law: {dep}"
    );
}
