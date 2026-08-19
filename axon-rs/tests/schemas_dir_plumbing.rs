//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.31.0 (D3 + D7 + D8) — `--schemas-dir` plumbing into deploy_handler.
//!
//! This pack proves the load-bearing wire that activates the v1.31.0
//! compile-time-declared store schema as the authoritative shape at
//! deploy:
//!
//! section 1 — D5 absolute backwards-compat. With `schemas_dir: None`, the
//!         deploy handler runs the v1.37.0 verification path verbatim.
//!         An adopter who never sets `--schemas-dir` observes ZERO
//!         behavior change.
//!
//! section 2 — Empty directory is a no-op. `--schemas-dir <empty_dir>` is
//!         valid (`load_and_merge_manifests` is total); the resulting
//!         empty manifest threads cleanly through `verify_postgres_…
//!         with_manifest(Some(&empty))`.
//!
//! section 3 — Missing directory is a no-op. A non-existent path is the
//!         same as an empty directory — no manifest files discovered.
//! The deploy proceeds; the behavior is identical to section 2.
//!
//! section 4 — D3+D8 hash-mismatch surface. A manifest file whose declared
//!         `content_hash` disagrees with its canonical SHA-256 raises
//!         axon-T805 (parse-stage). The deploy handler returns a
//!         structured 200 OK body with `success: false`, `phase:
//!         "store_schema_manifest_load"`, `d_letter: "D3+D8"`, and the
//!         offending `schemas_dir` echoed back to the operator.
//!
//! section 5 — D3+D8 duplicate-store surface. Two manifest files declaring
//!         the SAME `<namespace>.<store>` key raise
//!         `ManifestError::DuplicateStore`. The handler returns the
//! same structured shape as section 4 — manifest-load errors are a
//!         single observable failure mode for the operator (one phase,
//!         one D-letter pair).
//!
//! Observation technique: an `.axon` source with NO postgres stores
//! (a `step`-only flow) bypasses the verification logic on the
//! happy path while still flowing through `load_and_merge_manifests`.
//! The first observable bit is whether the deploy returns
//! `success: true` (the manifest loaded clean) or the structured 400-
//! shape with `success: false`. Infra-free — no Postgres needed.

use axon::axon_server::{build_router, ServerConfig};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

// ─── Fixtures ────────────────────────────────────────────────────────

/// Minimal `.axon` source — no axonstores, so the deploy path runs the
/// full pipeline (manifest load + verify) and exercises the plumbing
/// without needing a live Postgres. Mirrors the canonical shape used
/// by v1.31.0: a `flow … () -> Unit` with one `step … { ask: … }`,
/// behind a streaming `axonendpoint` so the deploy commits before any
/// runtime request hits the wire.
const PURE_FLOW_SRC: &str = "flow Demo() -> Unit {\n\
    step Reply { ask: \"hello\" output: Stream<Token> }\n\
}\n\
axonendpoint DemoE { public: true method: POST path: \"/demo\" \
execute: Demo backend: stub transport: sse }";

fn server_cfg(schemas_dir: Option<&str>) -> ServerConfig {
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
        schemas_dir: schemas_dir.map(|s| s.to_string()),
    }
}

async fn deploy_json(app: &axum::Router, src: &str) -> (StatusCode, serde_json::Value) {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/deploy")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::json!({ "source": src }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    (status, json)
}

/// Unique tempdir under the OS temp root for one test case. Caller is
/// responsible for cleaning it up via the `_TempDir` RAII guard.
struct _TempDir {
    path: std::path::PathBuf,
}
impl _TempDir {
    fn new(tag: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "axon-v1.31.0-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        ));
        std::fs::create_dir_all(&path).expect("create tempdir");
        Self { path }
    }
    fn path(&self) -> &std::path::Path {
        &self.path
    }
    fn write(&self, name: &str, body: &str) {
        let f = self.path.join(name);
        std::fs::write(f, body).expect("write tempdir file");
    }
}
impl Drop for _TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// ─── section 1 — D5 absolute backwards-compat: no schemas_dir is a no-op ───

#[tokio::test]
async fn s1_no_schemas_dir_preserves_pre_38_behavior() {
    let app = build_router(server_cfg(None));
    let (status, json) = deploy_json(&app, PURE_FLOW_SRC).await;
    assert_eq!(status, StatusCode::OK, "deploy http status: {json}");
    assert_eq!(
        json["success"], true,
        "D5 — without --schemas-dir, deploy of pure-step source MUST succeed verbatim: {json}",
    );
}

// ─── section 2 — empty directory is a clean no-op ──────────────────────────

#[tokio::test]
async fn s2_empty_schemas_dir_is_a_no_op() {
    let dir = _TempDir::new("s2-empty");
    let app = build_router(server_cfg(Some(dir.path().to_str().unwrap())));
    let (status, json) = deploy_json(&app, PURE_FLOW_SRC).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json["success"], true,
        "empty --schemas-dir MUST succeed (manifest is empty, no PG stores declared): {json}",
    );
}

// ─── section 3 — non-existent directory threads as "no manifests" ──────────

#[tokio::test]
async fn s3_missing_schemas_dir_is_a_no_op() {
    let mut missing = std::env::temp_dir();
    missing.push(format!(
        "axon-v1.31.0-s3-does-not-exist-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    ));
    assert!(
        !missing.exists(),
        "test invariant: this path must not pre-exist",
    );
    let app = build_router(server_cfg(Some(missing.to_str().unwrap())));
    let (status, json) = deploy_json(&app, PURE_FLOW_SRC).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json["success"], true,
        "missing --schemas-dir MUST be total (a non-existent dir resolves to empty manifest set): {json}",
    );
}

// ─── section 4 — content_hash mismatch surfaces as structured 200-OK error ─

#[tokio::test]
async fn s4_content_hash_mismatch_returns_structured_error() {
    let dir = _TempDir::new("s4-hash");
    // A manifest whose `content_hash` is wrong by construction —
    // canonical SHA-256 of the body (sans the content_hash field) will
    // disagree, raising axon-T805.
    let bad_manifest = r#"{
        "version": 1,
        "content_hash": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "stores": {
            "tenants": {
                "columns": {
                    "tenant_id": { "type": "Uuid", "primary_key": true }
                }
            }
        }
    }"#;
    dir.write("bad.axon-schema.json", bad_manifest);
    let app = build_router(server_cfg(Some(dir.path().to_str().unwrap())));
    let (status, json) = deploy_json(&app, PURE_FLOW_SRC).await;
    assert_eq!(status, StatusCode::OK, "handler returns 200 with structured body");
    assert_eq!(
        json["success"], false,
        "hash-mismatch MUST fail the deploy: {json}",
    );
    assert_eq!(
        json["phase"], "store_schema_manifest_load",
        "structured phase must be `store_schema_manifest_load`: {json}",
    );
    assert_eq!(
        json["d_letter"], "D3+D8",
        "structured d_letter must be `D3+D8`: {json}",
    );
    let schemas_dir_echoed = json["schemas_dir"]
        .as_str()
        .expect("schemas_dir must be echoed in the error");
    assert_eq!(
        schemas_dir_echoed,
        dir.path().to_str().unwrap(),
        "schemas_dir echo must equal the operator-supplied path verbatim",
    );
}

// ─── section 5 — duplicate store across files is structurally surfaced ─────

#[tokio::test]
async fn s5_duplicate_store_returns_structured_error() {
    let dir = _TempDir::new("s5-dup");
    // Two manifest files declaring the SAME store name (`tenants`).
    // `load_and_merge_manifests` rejects this with a typed
    // `DuplicateStore` error.
    let file_a_body = r#"{
        "version": 1,
        "stores": {
            "tenants": {
                "columns": {
                    "tenant_id": { "type": "Uuid", "primary_key": true }
                }
            }
        }
    }"#;
    let file_b_body = r#"{
        "version": 1,
        "stores": {
            "tenants": {
                "columns": {
                    "tenant_id": { "type": "Uuid", "primary_key": true },
                    "tier":      { "type": "Text" }
                }
            }
        }
    }"#;
    dir.write("a.axon-schema.json", file_a_body);
    dir.write("b.axon-schema.json", file_b_body);
    let app = build_router(server_cfg(Some(dir.path().to_str().unwrap())));
    let (status, json) = deploy_json(&app, PURE_FLOW_SRC).await;
    assert_eq!(status, StatusCode::OK, "handler returns 200 with structured body");
    assert_eq!(
        json["success"], false,
        "duplicate-store MUST fail the deploy: {json}",
    );
    assert_eq!(json["phase"], "store_schema_manifest_load");
    assert_eq!(json["d_letter"], "D3+D8");
    let err_msg = json["error"]
        .as_str()
        .expect("error field present");
    assert!(
        err_msg.contains("tenants"),
        "duplicate-store error MUST name the offending store: {err_msg}",
    );
}

// ─── section 6 — happy-path manifest with no PG stores in source ───────────

#[tokio::test]
async fn s6_well_formed_manifest_threads_clean_when_no_pg_stores_referenced() {
    let dir = _TempDir::new("s6-clean");
    // A correctly-hashed manifest. We compute the hash by emitting the
    // canonical form via the same code path the verifier reads, so the
    // round-trip is guaranteed sound (D11 anchor invariant).
    let body_no_hash = r#"{"stores":{"audit_log":{"columns":{"entry_id":{"primary_key":true,"type":"Uuid"}}}},"version":1}"#;
    // Use SHA-256 of the canonical form computed by parsing+re-emitting.
    let manifest = axon::store_schema_manifest::Manifest::parse_json(body_no_hash)
        .expect("parse synthetic manifest");
    // `false` ≡ omit the `content_hash` field from the canonical form
    // — that's the form the verifier hashes (see `verify_content_hash`).
    let canonical = manifest.canonical_serialize(false);
    let expected_hash = axon::store_schema_manifest::sha256_hex(canonical.as_bytes());
    // Emit a parseable file whose declared content_hash matches the
    // canonical hash. The contract on disk requires the `sha256:`
    // prefix on the hex digest (per the parser invariant; see
    // `store_schema_manifest.rs` doc string).
    let file_body = format!(
        r#"{{
            "version": 1,
            "content_hash": "sha256:{expected_hash}",
            "stores": {{
                "audit_log": {{
                    "columns": {{
                        "entry_id": {{ "type": "Uuid", "primary_key": true }}
                    }}
                }}
            }}
        }}"#,
    );
    dir.write("good.axon-schema.json", &file_body);
    let app = build_router(server_cfg(Some(dir.path().to_str().unwrap())));
    let (status, json) = deploy_json(&app, PURE_FLOW_SRC).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json["success"], true,
        "well-formed manifest with no referenced PG stores MUST deploy clean: {json}",
    );
}
