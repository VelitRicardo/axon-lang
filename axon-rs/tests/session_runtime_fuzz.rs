//! v2.3.0 — runtime-layer fuzz lane: the v2.3.0 sealed-runtime
//! round-trip invariant.
//!
//! Property: for **any** mid-protocol `SessionRuntime`, the seal →
//! JSON-bytes → `from_bytes` → `resume(declared)` cycle reconstructs a
//! runtime whose cursor + credit window equal the original. Driven by
//! the same deterministic 64-bit LCG + 50-seed corpus as the
//! algebra-layer fuzz in `axon-frontend/tests/session_runtime_fuzz.rs`.
//!
//! Algebra-layer invariants (duality involutivity, connection-law
//! symmetry, credit_analyse totality, polarity dual-symmetry, multiparty
//! projection totality) live in that sibling file because `axon-frontend`
//! cannot reach the runtime crate — the dep graph goes runtime →
//! frontend, not the reverse.

#![allow(clippy::needless_return)]

use std::collections::BTreeMap;

use axon::session::SessionType;
use axon::session_runtime::{SealedRuntime, SessionRuntime};

// ── Deterministic PRNG (matches axon-frontend's fuzz LCG) ────────────────

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn next_in(&mut self, lo: u64, hi: u64) -> u64 {
        let span = hi - lo + 1;
        lo + (self.next_u64() % span)
    }
    fn next_bool(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
}

const SEEDS: &[u64] = &[
    0x1, 0x3, 0x7, 0xd, 0xf, 0x11, 0x13, 0x17, 0x1d, 0x1f, 0x25, 0x29, 0x2b, 0x2f, 0x35, 0x3b,
    0x3d, 0x43, 0x47, 0x49, 0x4f, 0x53, 0x59, 0x61, 0x65, 0x67, 0x6b, 0x6d, 0x71, 0x7f, 0x83, 0x89,
    0x8b, 0x95, 0x97, 0x9d, 0xa3, 0xa7, 0xad, 0xb3, 0xb5, 0xbf, 0xc1, 0xc5, 0xc7, 0xd3, 0xdf, 0xe3,
    0xe5, 0xe9,
];

const PAYLOADS: &[&str] = &["A", "B", "Msg", "Token", "T"];
const LABELS: &[&str] = &["ask", "done", "more", "ok"];

fn random_session(rng: &mut Lcg, depth: u8) -> SessionType {
    random_session_in(rng, depth, &[])
}

fn random_session_in(rng: &mut Lcg, depth: u8, in_scope: &[String]) -> SessionType {
    if depth == 0 {
        if !in_scope.is_empty() && rng.next_bool() {
            let idx = rng.next_in(0, in_scope.len() as u64 - 1) as usize;
            return SessionType::var(in_scope[idx].clone());
        }
        return SessionType::End;
    }
    match rng.next_in(0, 5) {
        0 => SessionType::End,
        1 => {
            let payload = PAYLOADS[rng.next_in(0, PAYLOADS.len() as u64 - 1) as usize];
            SessionType::send(payload, random_session_in(rng, depth - 1, in_scope))
        }
        2 => {
            let payload = PAYLOADS[rng.next_in(0, PAYLOADS.len() as u64 - 1) as usize];
            SessionType::recv(payload, random_session_in(rng, depth - 1, in_scope))
        }
        3 | 4 => {
            let arm_count = rng.next_in(1, 3) as usize;
            let mut arms: BTreeMap<String, SessionType> = BTreeMap::new();
            for _ in 0..arm_count {
                let label = LABELS[rng.next_in(0, LABELS.len() as u64 - 1) as usize].to_string();
                arms.entry(label).or_insert_with(|| {
                    random_session_in(rng, depth.saturating_sub(1), in_scope)
                });
            }
            if rng.next_bool() {
                SessionType::Select(arms)
            } else {
                SessionType::Branch(arms)
            }
        }
        _ => {
            let var = format!("X{}", in_scope.len());
            let mut scope = in_scope.to_vec();
            scope.push(var.clone());
            let body = random_session_in(rng, depth - 1, &scope);
            SessionType::rec(var, body)
        }
    }
}

#[test]
fn sealed_runtime_round_trips_via_json_for_random_schemas() {
    // v2.3.0 invariant: any non-End cursor round-trips through the
    // wire (JSON bytes) and resumes against its own schema with equal
    // cursor + equal credit window.
    let mut any_round_trip = false;
    for &seed in SEEDS {
        let mut rng = Lcg::new(seed);
        for _ in 0..10 {
            let depth = rng.next_in(1, 3) as u8;
            let schema = random_session(&mut rng, depth);
            // Skip End — no residual to seal.
            if matches!(schema, SessionType::End) {
                continue;
            }
            let budget = if rng.next_bool() {
                Some(rng.next_in(1, 4))
            } else {
                None
            };
            let runtime = SessionRuntime::new(schema.clone(), budget);
            let Some(sealed) = runtime.seal() else {
                continue; // head-unfolded into End
            };
            any_round_trip = true;
            let bytes = sealed.to_bytes();
            let recovered = SealedRuntime::from_bytes(&bytes)
                .unwrap_or_else(|e| panic!("bytes ↦ SealedRuntime at seed=0x{seed:x}: {e}"));
            assert_eq!(recovered, sealed, "round-trip equality at seed=0x{seed:x}");
            let resumed = SessionRuntime::resume(recovered, &schema).unwrap_or_else(|e| {
                panic!("resume(declared) at seed=0x{seed:x} for {schema}: {e}")
            });
            assert_eq!(
                resumed.cursor(),
                runtime.cursor(),
                "resumed cursor diverges at seed=0x{seed:x} for {schema}"
            );
            assert_eq!(
                resumed.credit(),
                runtime.credit(),
                "resumed credit window diverges at seed=0x{seed:x}"
            );
        }
    }
    // Sanity — across 50×10 = 500 attempts, at least one non-End cursor
    // should have round-tripped (otherwise the fuzz exercised nothing
    // meaningful). The seed corpus is wide enough that this always holds.
    assert!(any_round_trip, "fuzz exercised at least one non-End cursor");
}

#[test]
fn schema_mismatch_resumes_are_rejected_cleanly() {
    // A snapshot from one schema MUST NOT resume against an unrelated
    // schema — the v2.3.0 `SchemaMismatch` gate is non-trivially exercised.
    for &seed in SEEDS {
        let mut rng = Lcg::new(seed);
        for _ in 0..6 {
            let depth_a = rng.next_in(1, 3) as u8;
            let schema_a = random_session(&mut rng, depth_a);
            let depth_b = rng.next_in(1, 3) as u8;
            let schema_b = random_session(&mut rng, depth_b);
            if matches!(schema_a, SessionType::End) || schema_a.equiv(&schema_b) {
                continue; // skip — End has no seal; equiv schemas legitimately resume
            }
            let runtime = SessionRuntime::new(schema_a.clone(), None);
            let Some(sealed) = runtime.seal() else { continue };
            // The cross-schema resume MUST be a typed error, not a panic.
            let outcome = SessionRuntime::resume(sealed, &schema_b);
            assert!(
                outcome.is_err(),
                "resume across distinct schemas must reject at seed=0x{seed:x}: \
                 a={schema_a} vs b={schema_b}"
            );
        }
    }
}
