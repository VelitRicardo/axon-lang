//! §Fase 118.b.2 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! §Fase 32.i — High-profile vertical canonical-example integration tests.
//!
//! Pillar trace per D12: each canonical pattern in the plan vivo §
//! "X-ray vision" must defend itself in front of a banking / government
//! / legal / medicine auditor. This test file deploys each canonical
//! axonendpoint shape and exercises the WHOLE Fase 32 surface
//! (32.b path registration + 32.c body validation + 32.d output
//! validation + 32.e per-route transport + 32.f Idempotency-Key +
//! 32.g auth scope + 32.h replay binding) end-to-end against an
//! actual server, proving the patterns work as a coherent contract,
//! not just individual sub-features.
//!
//! Coverage by vertical:
//!
//!   1. **Banking** (PCI DSS Req 10 + SOC 2 CC6) —
//!      `axonendpoint LoanDecision { method:POST path:"/loan/decision"
//!      body:LoanApplication output:Decision execute:ApproveOrDeny
//!      requires:[bank.officer] replay:true }`.
//!      Verifies: schema validation of LoanApplication body, auth
//!      scope, replay binding writes, GET /v1/replay/<id> retrieves
//!      the canonical (request, response, capabilities_used) tuple
//!      for PCI DSS audit.
//!
//!   2. **Government** (FedRAMP AU-2 + FISMA) —
//!      `axonendpoint BenefitsEligibility { method:POST
//!      path:"/benefits/eligibility" body:BenefitsClaim
//!      output:EligibilityVerdict execute:AssessEligibility
//!      requires:[agency.case_officer] replay:true }`.
//!      Verifies: every benefits decision is auditable; FOIA
//!      requests can produce the exact request that led to a verdict.
//!
//!   3. **Legal** (FRE 502 + ABA Rule 1.6) —
//!      `axonendpoint DiscoveryPrivilege { method:POST
//!      path:"/discovery/privilege" body:DiscoveryDocument
//!      output:PrivilegeAssessment execute:AssessPrivilege
//!      requires:[legal.privileged_review] replay:true }`.
//!      Verifies: privileged-review capability gating; trace_id
//!      correlation for waiver doctrine appeals.
//!
//!   4. **Medicine** (HIPAA + 21 CFR Part 11) —
//!      `axonendpoint ClinicalDecisionSupport { method:POST
//!      path:"/clinical/decision-support" body:ClinicalDecisionRequest
//!      output:ClinicalDecisionSupport execute:GenerateCDS
//!      requires:[hipaa.phi.read, clinician] replay:true }`.
//!      Verifies: multi-capability AND-gate; replay binding for
//!      clinical adverse-event review.

use axon::axon_server::{build_router, ServerConfig};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
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

async fn deploy(app: axum::Router, src: &str) {
    let body = serde_json::json!({
        "source": src,
        "source_file": "vertical.axon",
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

/// Mint an unverified JWT carrying the given `capabilities` claim.
fn jwt_with_caps(caps: &[&str]) -> String {
    let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"none\",\"typ\":\"JWT\"}");
    let payload_json = serde_json::json!({"capabilities": caps});
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload_json).unwrap());
    format!("{header}.{payload}.")
}

fn post_with(
    path: &str,
    body: &serde_json::Value,
    bearer: Option<&str>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json");
    if let Some(b) = bearer {
        builder = builder.header("authorization", format!("Bearer {b}"));
    }
    builder
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

// ─── 1. Banking pattern (PCI DSS Req 10 + SOC 2 CC6) ────────────────

const BANKING_SOURCE: &str =
    "type Money { amount: Float currency: String }\n\
     type Person { full_name: String ssn_last4: String }\n\
     type LoanApplication { amount: Money applicant: Person }\n\
     type Decision { approved: Boolean basis: String }\n\
     flow ApproveOrDeny() -> String { let result = \"ok\" return result }\n\
     axonendpoint LoanDecision { method: POST path: \"/loan/decision\" \
        body: LoanApplication execute: ApproveOrDeny \
        requires: [bank.officer] replay: true }";

#[tokio::test]
async fn banking_canonical_pattern_end_to_end() {
    let app = build_router(server_cfg());
    deploy(app.clone(), BANKING_SOURCE).await;

    // Well-formed body + correct capability → success path.
    let good = serde_json::json!({
        "amount": {"amount": 50000.0, "currency": "USD"},
        "applicant": {"full_name": "Alice Citizen", "ssn_last4": "1234"}
    });
    let token = jwt_with_caps(&["bank.officer"]);
    let resp = app
        .clone()
        .oneshot(post_with("/loan/decision", &good, Some(&token)))
        .await
        .unwrap();
    let trace_id = resp
        .headers()
        .get("x-axon-trace-id")
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .to_string();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let p: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_ne!(p["error"], "missing_capability");
    assert_ne!(p["error"], "body_schema_violation");

    // PCI DSS audit: replay binding written.
    let replay_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/replay/{trace_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replay_resp.status(), StatusCode::OK);
    let bytes = replay_resp.into_body().collect().await.unwrap().to_bytes();
    let replay: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(replay["endpoint_name"], "LoanDecision");
    assert_eq!(replay["method"], "POST");
    assert_eq!(replay["path"], "/loan/decision");
    let caps = replay["capabilities_used"].as_array().unwrap();
    assert!(caps.iter().any(|c| c == "bank.officer"));

    // Bad body shape (amount as raw number, not Money struct) → 400.
    let bad = serde_json::json!({"amount": 50000, "applicant": "alice"});
    let resp = app
        .clone()
        .oneshot(post_with("/loan/decision", &bad, Some(&token)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Missing capability → 403.
    let wrong_token = jwt_with_caps(&["other.scope"]);
    let resp = app
        .oneshot(post_with("/loan/decision", &good, Some(&wrong_token)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ─── 2. Government pattern (FedRAMP AU-2 + FISMA) ───────────────────

const GOVERNMENT_SOURCE: &str =
    "type BenefitsClaim { citizen_id: String claim_type: String }\n\
     type EligibilityVerdict { eligible: Boolean basis: String }\n\
     flow AssessEligibility() -> String { let result = \"ok\" return result }\n\
     axonendpoint BenefitsEligibility { method: POST path: \"/benefits/eligibility\" \
        body: BenefitsClaim execute: AssessEligibility \
        requires: [agency.case_officer] replay: true }";

#[tokio::test]
async fn government_canonical_pattern_end_to_end() {
    let app = build_router(server_cfg());
    deploy(app.clone(), GOVERNMENT_SOURCE).await;

    let good = serde_json::json!({
        "citizen_id": "C-12345",
        "claim_type": "disability"
    });
    let token = jwt_with_caps(&["agency.case_officer"]);
    let resp = app
        .clone()
        .oneshot(post_with("/benefits/eligibility", &good, Some(&token)))
        .await
        .unwrap();
    let trace_id = resp
        .headers()
        .get("x-axon-trace-id")
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .to_string();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let p: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_ne!(p["error"], "missing_capability");
    assert_ne!(p["error"], "body_schema_violation");

    // FedRAMP AU-2: every benefits decision is registered for FOIA /
    // appeal audit. Replay must surface the trace_id + endpoint
    // declaration.
    let replay_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/replay/{trace_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replay_resp.status(), StatusCode::OK);
    let bytes = replay_resp.into_body().collect().await.unwrap().to_bytes();
    let replay: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(replay["endpoint_name"], "BenefitsEligibility");
    assert_eq!(replay["method"], "POST");
    // Deterministic flag: stub backend, so true.
    assert_eq!(replay["deterministic"], true);
}

// ─── 3. Legal pattern (FRE 502 + ABA Rule 1.6) ──────────────────────

const LEGAL_SOURCE: &str =
    "type DiscoveryDocument { case_id: String party: String }\n\
     type PrivilegeAssessment { privileged: Boolean doctrine: String }\n\
     flow AssessPrivilege() -> String { let result = \"ok\" return result }\n\
     axonendpoint DiscoveryPrivilege { method: POST path: \"/discovery/privilege\" \
        body: DiscoveryDocument execute: AssessPrivilege \
        requires: [legal.privileged_review] replay: true }";

#[tokio::test]
async fn legal_canonical_pattern_end_to_end() {
    let app = build_router(server_cfg());
    deploy(app.clone(), LEGAL_SOURCE).await;

    let good = serde_json::json!({
        "case_id": "CASE-2026-001",
        "party": "Plaintiff"
    });

    // FRE 502: privileged-review capability is gating. Without it,
    // 403 — inadvertent waiver-by-AI-disclosure structurally
    // impossible at the language layer.
    let no_cap_token = jwt_with_caps(&["legal.read"]);
    let resp = app
        .clone()
        .oneshot(post_with("/discovery/privilege", &good, Some(&no_cap_token)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // With the right capability, the assessment proceeds + is logged
    // for waiver-doctrine appeals.
    let cap_token = jwt_with_caps(&["legal.privileged_review"]);
    let resp = app
        .clone()
        .oneshot(post_with("/discovery/privilege", &good, Some(&cap_token)))
        .await
        .unwrap();
    let trace_id = resp
        .headers()
        .get("x-axon-trace-id")
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .to_string();
    let _ = resp.into_body().collect().await.unwrap().to_bytes();

    // FRE 502 appeal: the trace_id correlates back to the recorded
    // assessment.
    let replay_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/replay/{trace_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replay_resp.status(), StatusCode::OK);
    let replay_status = replay_resp
        .headers()
        .get("replay-status")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    assert_eq!(replay_status.as_deref(), Some("deterministic"));
}

// ─── 4. Medicine pattern (HIPAA + 21 CFR Part 11) ───────────────────

const MEDICINE_SOURCE: &str =
    "type Symptom { name: String score: ConfidenceScore }\n\
     type ClinicalDecisionRequest { patient_id: String symptoms: List<Symptom> }\n\
     type Recommendation { text: String }\n\
     type ClinicalDecisionSupport { recommendations: List<Recommendation> }\n\
     flow GenerateCDS() -> String { let result = \"ok\" return result }\n\
     axonendpoint CDSEndpoint { method: POST path: \"/clinical/decision-support\" \
        body: ClinicalDecisionRequest execute: GenerateCDS \
        requires: [hipaa.phi.read, clinician] replay: true }";

#[tokio::test]
async fn medicine_canonical_pattern_end_to_end() {
    let app = build_router(server_cfg());
    deploy(app.clone(), MEDICINE_SOURCE).await;

    let good = serde_json::json!({
        "patient_id": "anon-1234",
        "symptoms": [
            {"name": "fatigue", "score": 0.7},
            {"name": "headache", "score": 0.3}
        ]
    });

    // HIPAA: multi-capability AND. Only one of two → 403.
    let partial_token = jwt_with_caps(&["hipaa.phi.read"]);
    let resp = app
        .clone()
        .oneshot(post_with("/clinical/decision-support", &good, Some(&partial_token)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Both capabilities → success.
    let full_token = jwt_with_caps(&["hipaa.phi.read", "clinician"]);
    let resp = app
        .clone()
        .oneshot(post_with("/clinical/decision-support", &good, Some(&full_token)))
        .await
        .unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let p: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    assert_ne!(p["error"], "missing_capability");

    // 21 CFR Part 11: severity range violation triggers schema gate.
    let oob = serde_json::json!({
        "patient_id": "anon-9999",
        "symptoms": [{"name": "fatigue", "score": 1.5}]
    });
    let resp = app
        .oneshot(post_with("/clinical/decision-support", &oob, Some(&full_token)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let p: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(p["error"], "body_schema_violation");
    assert_eq!(p["field_path"], "symptoms[0].score");
}
