//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v1.23.0 — D12 robustness fuzz for the v1.23.0 REST surface (Rust side).
//!
//! Mirror of `tests/test_fase32_fuzz.py`. D12 budget extended to v1.23.0:
//! every public predicate the runtime exposes on the dynamic-route
//! surface MUST be **total + non-panicking** over its documented input
//! domain.
//!
//! Surfaces under fuzz:
//!
//!   1. `validate_body` (32.c + 32.d response side).
//!   2. `is_valid_capability_slug` (32.g).
//!   3. `resolve_replay_enabled` (32.h) — pure boolean function.
//!   4. `IdempotencyStore::lookup` + `insert` round-trip (32.f).
//!   5. `classify_dynamic_route_wire` (32.e) — 5-input truth-table
//!      total function.
//!   6. `AxonendpointReplayLog` append/get round-trip (32.h).
//!
//! The invariant: every public function returns its documented type
//! (Result / bool / Option / enum variant) for ANY input. NEVER panics
//! with an uncaught exception, infinite-loop, or stack-overflow.
//!
//! Pillar trace per D12:
//!   - MATHEMATICS — each function is total over its documented domain.
//!   - LOGIC      — output is always in the closed set the contract
//!                   declares.
//!   - COMPUTING  — defensive; no adversarial input shape crashes.
//!
//! Seeded RNG produces deterministic runs — a regression reproduces
//! verbatim with the same `seed` value.

use std::collections::HashMap;

use axon::auth_scope::{check_capabilities, is_valid_capability_slug, AuthVerdict};
use axon::axon_server::{classify_dynamic_route_wire, DynamicRouteWire};
use axon::axonendpoint_replay::{
    is_backend_deterministic, resolve_replay_enabled, AxonendpointReplayEntry,
    AxonendpointReplayLog,
};
use axon::idempotency::{IdempotencyCacheKey, IdempotencyEntry, IdempotencyStore, IdempotencyVerdict};
use axon::route_schema::{
    builtin_range, fmt_f64, validate_body, FieldSchema, TypeSchema, BUILTIN_PRIMITIVES,
};

// ── Tiny seeded LCG so the fuzz is deterministic without pulling rand ──

struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self { state: seed.wrapping_add(0x9E37_79B9_7F4A_7C15) }
    }
    fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    fn range(&mut self, lo: usize, hi_inclusive: usize) -> usize {
        let span = (hi_inclusive - lo + 1) as u64;
        (self.next_u64() % span) as usize + lo
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.range(0, items.len() - 1)]
    }
    fn bool(&mut self) -> bool {
        (self.next_u64() & 1) == 0
    }
    fn ascii_string(&mut self, max_len: usize) -> String {
        let len = self.range(0, max_len);
        let alphabet =
            b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-@/ ";
        (0..len)
            .map(|_| {
                let idx = self.range(0, alphabet.len() - 1);
                alphabet[idx] as char
            })
            .collect()
    }
    fn json_value(&mut self) -> serde_json::Value {
        match self.range(0, 6) {
            0 => serde_json::Value::Null,
            1 => serde_json::Value::Bool(self.bool()),
            2 => serde_json::json!(self.range(0, 100) as i64 - 50),
            3 => serde_json::json!(((self.next_u64() % 1000) as f64) / 10.0 - 50.0),
            4 => serde_json::Value::String(self.ascii_string(8)),
            5 => serde_json::json!([self.range(0, 100) as i64]),
            _ => serde_json::json!({"k": self.range(0, 100) as i64}),
        }
    }
}

const TYPE_NAMES: &[&str] = &[
    "String", "Integer", "Float", "Boolean", "Duration", "Any",
    "RiskScore", "ConfidenceScore", "SentimentScore", "List", "NotDeclared",
];

const HTTP_METHODS: &[&str] = &[
    "GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS", "CONNECT", "TRACE",
    "BOGUS", "",
];

// ─── section 1 — validate_body never panics ────────────────────────────────

#[test]
fn fuzz_validate_body_never_panics_100_seeds_10_iters() {
    for seed in 0u64..100 {
        let mut rng = Lcg::new(seed);
        for _iter in 0..10 {
            let body = rng.json_value();
            let type_name = (*rng.pick(TYPE_NAMES)).to_string();
            let mut table = HashMap::new();
            if rng.bool() {
                table.insert(
                    "Custom".to_string(),
                    TypeSchema {
                        name: "Custom".to_string(),
                        fields: vec![FieldSchema {
                            name: "x".to_string(),
                            type_name: (*rng.pick(TYPE_NAMES)).to_string(),
                            generic_param: if rng.bool() { "String".to_string() } else { String::new() },
                            optional: rng.bool(),
                        }],
                        range: None,
                    },
                );
            }
            // The contract is total — we just need to verify the call
            // returns; a panic would abort the test process.
            let _ = validate_body(&body, &type_name, &table);
        }
    }
}

#[test]
fn fuzz_validate_body_d9_empty_type_always_ok() {
    let mut rng = Lcg::new(0xDEAD_BEEF);
    let empty_table = HashMap::new();
    for _ in 0..1000 {
        let body = rng.json_value();
        assert!(
            validate_body(&body, "", &empty_table).is_ok(),
            "D9 backwards-compat: empty type_name must short-circuit to Ok for ANY body"
        );
    }
}

#[test]
fn fuzz_validate_body_any_always_ok() {
    let mut rng = Lcg::new(0xCAFE_BABE);
    let empty_table = HashMap::new();
    for _ in 0..1000 {
        let body = rng.json_value();
        assert!(
            validate_body(&body, "Any", &empty_table).is_ok(),
            "`Any` accepts ANY body shape"
        );
    }
}

// ─── section 2 — is_valid_capability_slug is total + returns bool ──────────

#[test]
fn fuzz_capability_slug_validator_total_over_random_strings() {
    for seed in 0u64..100 {
        let mut rng = Lcg::new(seed.wrapping_mul(7919));
        for _iter in 0..10 {
            let slug = rng.ascii_string(20);
            // Predicate is total — calling it just returns a bool.
            let _ = is_valid_capability_slug(&slug);
        }
    }
}

#[test]
fn fuzz_capability_slug_validator_idempotent() {
    for seed in 0u64..100 {
        let mut rng = Lcg::new(seed.wrapping_add(13));
        for _iter in 0..10 {
            let slug = rng.ascii_string(20);
            assert_eq!(
                is_valid_capability_slug(&slug),
                is_valid_capability_slug(&slug),
                "predicate must be pure / idempotent"
            );
        }
    }
}

// ─── section 3 — check_capabilities never panics ───────────────────────────

#[test]
fn fuzz_check_capabilities_never_panics() {
    for seed in 0u64..100 {
        let mut rng = Lcg::new(seed.wrapping_add(31));
        for _iter in 0..10 {
            let n_decl = rng.range(0, 5);
            let n_have = rng.range(0, 5);
            let declared: Vec<String> = (0..n_decl).map(|_| rng.ascii_string(8)).collect();
            let have: Vec<String> = (0..n_have).map(|_| rng.ascii_string(8)).collect();
            // Must return AuthVerdict variant — never panic.
            let verdict = check_capabilities(&declared, &have);
            match verdict {
                AuthVerdict::Allow | AuthVerdict::Deny { .. } => {}
            }
        }
    }
}

// ─── section 4 — resolve_replay_enabled is total ───────────────────────────

#[test]
fn fuzz_resolve_replay_enabled_total() {
    for seed in 0u64..100 {
        let mut rng = Lcg::new(seed.wrapping_add(73));
        for _iter in 0..10 {
            let method = (*rng.pick(HTTP_METHODS)).to_string();
            let explicit = rng.bool();
            let replay = rng.bool();
            let result = resolve_replay_enabled(&method, explicit, replay);
            // When explicit, declared value MUST win regardless of method.
            if explicit {
                assert_eq!(
                    result, replay,
                    "explicit declaration must win (method={method}, explicit={explicit}, replay={replay})"
                );
            }
        }
    }
}

// ─── section 5 — classify_dynamic_route_wire 6-input truth table ───────────
//
// v1.27.1 — extended to 6 inputs with the
// `has_algebraic_stream_effect` predicate.

#[test]
fn fuzz_classify_dynamic_route_wire_total_over_6_inputs() {
    let transports = ["sse", "json", "ndjson", "", "bogus"];
    let implicits = ["sse", "json", ""];
    for seed in 0u64..100 {
        let mut rng = Lcg::new(seed.wrapping_add(127));
        for _iter in 0..10 {
            let transport = *rng.pick(&transports);
            let explicit = rng.bool();
            let implicit = *rng.pick(&implicits);
            let algebraic = rng.bool();
            let accept_sse = rng.bool();
            let strict = rng.bool();
            // Must return one of two enum variants — never panic.
            let wire = classify_dynamic_route_wire(
                transport, explicit, implicit, algebraic, accept_sse, strict,
            );
            match wire {
                DynamicRouteWire::Sse | DynamicRouteWire::Json => {}
            }
        }
    }
}

// ─── section 6 — IdempotencyStore lookup/insert round-trip ─────────────────

#[test]
fn fuzz_idempotency_store_round_trip_never_panics() {
    for seed in 0u64..100 {
        let mut rng = Lcg::new(seed.wrapping_add(199));
        let mut store = IdempotencyStore::default();
        for _iter in 0..10 {
            let body_bytes = rng.ascii_string(32).into_bytes();
            let hash = IdempotencyStore::hash_body(&body_bytes);
            let key = IdempotencyCacheKey {
                client_id: rng.ascii_string(8),
                endpoint_path: format!("/{}", rng.ascii_string(8)),
                idempotency_key: rng.ascii_string(8),
            };
            // Lookup before insert — must return Miss (or Conflict if
            // a prior insert collided, but with random keys this is
            // vanishingly unlikely).
            let verdict_before = store.lookup(&key, &hash);
            match verdict_before {
                IdempotencyVerdict::Miss
                | IdempotencyVerdict::Hit(_)
                | IdempotencyVerdict::Conflict { .. } => {}
            }
            // Insert and look up again.
            store.insert(
                key.clone(),
                IdempotencyEntry {
                    request_body_hash: hash,
                    status: 200,
                    content_type: "application/json".to_string(),
                    body: body_bytes.clone(),
                    inserted_at: std::time::Instant::now(),
                },
            );
            let verdict_after = store.lookup(&key, &hash);
            // Same key + same body must be Hit.
            assert!(
                matches!(verdict_after, IdempotencyVerdict::Hit(_)),
                "after insert, same-key + same-body lookup must Hit"
            );
        }
    }
}

// ─── section 7 — AxonendpointReplayLog round-trip ─────────────────────────

#[test]
fn fuzz_replay_log_round_trip_never_panics() {
    for seed in 0u64..100 {
        let mut rng = Lcg::new(seed.wrapping_add(257));
        let mut log = AxonendpointReplayLog::default();
        for _iter in 0..10 {
            let trace_id = format!("trace-{}-{}", seed, _iter);
            let request_body = rng.ascii_string(32).into_bytes();
            let response_body = rng.ascii_string(32).into_bytes();
            let entry = AxonendpointReplayEntry {
                trace_id: trace_id.clone(),
                timestamp_ms: 0,
                endpoint_name: rng.ascii_string(8),
                flow_name: rng.ascii_string(8),
                method: (*rng.pick(HTTP_METHODS)).to_string(),
                path: format!("/{}", rng.ascii_string(8)),
                client_id: rng.ascii_string(8),
                capabilities_used: vec![],
                request_body_hash_hex: AxonendpointReplayLog::hash_body_hex(&request_body),
                request_body,
                response_status: 200,
                response_body_hash_hex: AxonendpointReplayLog::hash_body_hex(&response_body),
                response_content_type: "application/json".to_string(),
                response_body,
                model_version: "fuzz".to_string(),
                deterministic: rng.bool(),
                step_audit: Vec::new(),
                runtime_warnings: Vec::new(),
            };
            log.append(entry);
            assert!(
                log.get(&trace_id).is_some(),
                "append + get round-trip must succeed for trace_id={trace_id}"
            );
        }
    }
}

// ─── section 8 — Cross-surface invariants ──────────────────────────────────

#[test]
fn fuzz_fmt_f64_is_pure() {
    let mut rng = Lcg::new(0x1234_5678);
    for _ in 0..1000 {
        let n = ((rng.next_u64() % 10000) as f64) / 100.0 - 50.0;
        assert_eq!(fmt_f64(n), fmt_f64(n), "fmt_f64 must be pure");
    }
}

#[test]
fn fuzz_builtin_range_total_over_random_names() {
    for seed in 0u64..100 {
        let mut rng = Lcg::new(seed.wrapping_add(311));
        for _iter in 0..10 {
            let name = rng.ascii_string(12);
            // Total function over arbitrary string input.
            let _ = builtin_range(&name);
        }
    }
}

#[test]
fn fuzz_is_backend_deterministic_total() {
    let mut rng = Lcg::new(0xFEED_FACE);
    for _ in 0..1000 {
        let backend = rng.ascii_string(12);
        let result = is_backend_deterministic(&backend);
        // Stub-only-true contract: anything other than literal "stub"
        // returns false. Verifies the closed-set invariant.
        assert_eq!(result, backend == "stub");
    }
}

// ─── section 9 — BUILTIN_PRIMITIVES is the closed set ──────────────────────

#[test]
fn fuzz_validate_body_with_builtin_primitives_anchor() {
    // For every primitive name, validate_body returns a result.
    // Closed-set invariant: BUILTIN_PRIMITIVES is the union of
    // accepted type-name strings for the primitive path.
    let mut rng = Lcg::new(0xBEEF_DEAD);
    for &prim in BUILTIN_PRIMITIVES {
        for _ in 0..50 {
            let body = rng.json_value();
            let empty_table = HashMap::new();
            // Just verify no panic.
            let _ = validate_body(&body, prim, &empty_table);
        }
    }
}
