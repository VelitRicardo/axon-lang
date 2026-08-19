//! v2.3.0 — deterministic fuzz lane for the session-type theory
//! (algebra layer — `axon-frontend` cannot reach the runtime, so the
//! sealed-runtime invariant runs in the sibling `axon-rs/tests/
//! session_type_fuzz.rs`).
//!
//! Five algebra-level property surfaces, each driven by a 64-bit LCG
//! over a closed 50-seed corpus (so a CI failure is exactly
//! reproducible from the seed):
//!
//! 1. **Duality involutivity** — `(S⊥)⊥ ≡ S` for any random closed
//! session type. The v2.3.0 connection-law cornerstone (paper section 3.2).
//! 2. **Connection-law symmetry** — `S.is_dual_to(S⊥)` ∧
//!    `S⊥.is_dual_to(S)`.
//! 3. **Credit-analyse totality** — `S.credit_analyse(budget)` is total
//!    (never panics) for any random `S` + budget, even when the
//!    sustainability fixpoint forbids the protocol.
//! 4. **Polarity dual-symmetry** — `S.projects_to_sse() ⇔
//!    S.dual().projects_to_sse_consumer()` for every random `S`.
//! 5. **Multiparty projection totality** — `g.project_all()` either
//!    returns Ok with one binding per declared role, or returns one of
//!    the typed `ProjectionError` reasons; never panics.
//! 6. **Two-role projection realises duality** — for `r1→r2:T1.r2→r1:T2`,
//!    the projections are mutually dual (Honda-Yoshida-Carbone soundness).

#![allow(clippy::needless_return)]

use std::collections::BTreeMap;

use axon_frontend::multiparty::GlobalType;
use axon_frontend::session::{Polarity, SessionType};

// ── section 0 — Deterministic PRNG ──────────────────────────────────────────────

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
        debug_assert!(hi >= lo);
        let span = hi - lo + 1;
        lo + (self.next_u64() % span)
    }
    fn next_bool(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
}

/// 50 distinct seeds — a regression on any one is a deterministic
/// reproducer (`SEEDS[i]` in the failure message points at the trace).
const SEEDS: &[u64] = &[
    0x1, 0x3, 0x7, 0xd, 0xf, 0x11, 0x13, 0x17, 0x1d, 0x1f, 0x25, 0x29, 0x2b, 0x2f, 0x35, 0x3b,
    0x3d, 0x43, 0x47, 0x49, 0x4f, 0x53, 0x59, 0x61, 0x65, 0x67, 0x6b, 0x6d, 0x71, 0x7f, 0x83, 0x89,
    0x8b, 0x95, 0x97, 0x9d, 0xa3, 0xa7, 0xad, 0xb3, 0xb5, 0xbf, 0xc1, 0xc5, 0xc7, 0xd3, 0xdf, 0xe3,
    0xe5, 0xe9,
];

// ── section 1 — Random session-type generator ───────────────────────────────────

const PAYLOADS: &[&str] = &["A", "B", "Msg", "Token", "T", "Q"];
const LABELS: &[&str] = &["ask", "done", "more", "ok", "cancel"];

fn random_session(rng: &mut Lcg, depth: u8) -> SessionType {
    random_session_in(rng, depth, &[])
}

/// `in_scope` carries the recursion-variable names currently bound — only
/// those can appear as `Var(_)` (keeps every generated type closed).
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
        3 => random_choice(rng, depth, in_scope, /*internal=*/ true),
        4 => random_choice(rng, depth, in_scope, /*internal=*/ false),
        _ => {
            let var = format!("X{}", in_scope.len());
            let mut scope = in_scope.to_vec();
            scope.push(var.clone());
            let body = random_session_in(rng, depth - 1, &scope);
            SessionType::rec(var, body)
        }
    }
}

fn random_choice(rng: &mut Lcg, depth: u8, in_scope: &[String], internal: bool) -> SessionType {
    let arm_count = rng.next_in(1, 3) as usize;
    let mut arms: BTreeMap<String, SessionType> = BTreeMap::new();
    for _ in 0..arm_count {
        let label = LABELS[rng.next_in(0, LABELS.len() as u64 - 1) as usize].to_string();
        arms.entry(label)
            .or_insert_with(|| random_session_in(rng, depth.saturating_sub(1), in_scope));
    }
    if internal {
        SessionType::Select(arms)
    } else {
        SessionType::Branch(arms)
    }
}

// ── section 2 — Random GlobalType generator ─────────────────────────────────────

const ROLES: &[&str] = &["Alice", "Bob", "Carol", "Dave"];

fn random_global(rng: &mut Lcg, depth: u8) -> GlobalType {
    random_global_in(rng, depth, &[])
}

fn random_global_in(rng: &mut Lcg, depth: u8, in_scope: &[String]) -> GlobalType {
    if depth == 0 {
        if !in_scope.is_empty() && rng.next_bool() {
            let idx = rng.next_in(0, in_scope.len() as u64 - 1) as usize;
            return GlobalType::var(in_scope[idx].clone());
        }
        return GlobalType::End;
    }
    match rng.next_in(0, 3) {
        0 => GlobalType::End,
        1 => {
            let (from, to) = distinct_roles(rng);
            let payload = PAYLOADS[rng.next_in(0, PAYLOADS.len() as u64 - 1) as usize];
            GlobalType::message(
                from,
                to,
                payload,
                random_global_in(rng, depth - 1, in_scope),
            )
        }
        2 => {
            let (from, to) = distinct_roles(rng);
            let arm_count = rng.next_in(1, 3) as usize;
            let mut arms: BTreeMap<String, GlobalType> = BTreeMap::new();
            for _ in 0..arm_count {
                let label = LABELS[rng.next_in(0, LABELS.len() as u64 - 1) as usize].to_string();
                arms.entry(label)
                    .or_insert_with(|| random_global_in(rng, depth.saturating_sub(1), in_scope));
            }
            GlobalType::choice(from, to, arms)
        }
        _ => {
            let var = format!("Y{}", in_scope.len());
            let mut scope = in_scope.to_vec();
            scope.push(var.clone());
            let body = random_global_in(rng, depth - 1, &scope);
            GlobalType::rec(var, body)
        }
    }
}

fn distinct_roles(rng: &mut Lcg) -> (String, String) {
    loop {
        let a = ROLES[rng.next_in(0, ROLES.len() as u64 - 1) as usize].to_string();
        let b = ROLES[rng.next_in(0, ROLES.len() as u64 - 1) as usize].to_string();
        if a != b {
            return (a, b);
        }
    }
}

// ── section 3 — Invariants under test ───────────────────────────────────────────

#[test]
fn duality_is_involutive_for_random_closed_session_types() {
    for &seed in SEEDS {
        let mut rng = Lcg::new(seed);
        for _ in 0..20 {
            let depth = rng.next_in(1, 4) as u8;
            let s = random_session(&mut rng, depth);
            let twice = s.dual().dual();
            assert!(
                twice.equiv(&s),
                "(S⊥)⊥ ≢ S at seed=0x{seed:x} for: {s}"
            );
        }
    }
}

#[test]
fn connection_law_holds_for_dual_pairs_in_both_directions() {
    for &seed in SEEDS {
        let mut rng = Lcg::new(seed);
        for _ in 0..20 {
            let depth = rng.next_in(1, 3) as u8;
            let s = random_session(&mut rng, depth);
            let t = s.dual();
            assert!(
                s.is_dual_to(&t),
                "S.is_dual_to(S⊥) must hold at seed=0x{seed:x} for: {s}"
            );
            assert!(
                t.is_dual_to(&s),
                "S⊥.is_dual_to(S) must hold at seed=0x{seed:x} for: {s}"
            );
        }
    }
}

#[test]
fn credit_analyse_is_total_over_random_sessions_and_budgets() {
    for &seed in SEEDS {
        let mut rng = Lcg::new(seed);
        for _ in 0..20 {
            let depth = rng.next_in(1, 3) as u8;
            let s = random_session(&mut rng, depth);
            let budget = rng.next_in(0, 8);
            // Totality = never panics. The verdict (Ok / SendAtZero /
            // BurstOverflow / LoopUnsustainable) is structurally part of
            // the contract; we don't pre-commit to which one fires.
            let _verdict = s.credit_analyse(budget);
        }
    }
}

#[test]
fn polarity_predicate_obeys_dual_symmetry() {
    // The section 4.4 identity Π↓(S)⊥ = Π↑(S⊥) projected to a predicate:
    // S.projects_to_sse() ⇔ S.dual().projects_to_sse_consumer().
    for &seed in SEEDS {
        let mut rng = Lcg::new(seed);
        for _ in 0..20 {
            let depth = rng.next_in(1, 3) as u8;
            let s = random_session(&mut rng, depth);
            assert_eq!(
                s.projects_to_sse(),
                s.dual().projects_to_sse_consumer(),
                "polarity symmetry violated at seed=0x{seed:x} for: {s}"
            );
            // Unified `has_polarity` agrees with the named predicates.
            assert_eq!(
                s.has_polarity(Polarity::Producer),
                s.projects_to_sse()
            );
            assert_eq!(
                s.has_polarity(Polarity::Consumer),
                s.projects_to_sse_consumer()
            );
        }
    }
}

#[test]
fn multiparty_projection_is_total_over_random_closed_global_types() {
    // project_all returns Ok with len = |roles()| OR Err(typed reason).
    // Never panics. (The Err arms are the gate doing its job.)
    for &seed in SEEDS {
        let mut rng = Lcg::new(seed);
        for _ in 0..20 {
            let depth = rng.next_in(1, 3) as u8;
            let g = random_global(&mut rng, depth);
            let role_set = g.roles();
            match g.project_all() {
                Ok(projection) => {
                    assert_eq!(
                        projection.len(),
                        role_set.len(),
                        "project_all must yield one binding per role at seed=0x{seed:x}: {g:?}"
                    );
                    for r in &role_set {
                        assert!(
                            projection.contains_key(r),
                            "role {r} missing from projection at seed=0x{seed:x}"
                        );
                    }
                }
                Err(_e) => {
                    // The gate rejected — fine, the invariant is just
                    // "never panics + every Ok is structurally sound".
                }
            }
        }
    }
}

#[test]
fn projection_for_a_dual_pair_realises_the_connection_law() {
    // For any two-role linear protocol `r1 -> r2 : T1 . r2 -> r1 : T2`,
    // the projections of `r1` and `r2` MUST be dual under is_dual_to.
    // This pins the Honda-Yoshida-Carbone soundness theorem to a sample.
    for &seed in SEEDS {
        let mut rng = Lcg::new(seed);
        for _ in 0..10 {
            let (r1, r2) = distinct_roles(&mut rng);
            let t1 = PAYLOADS[rng.next_in(0, PAYLOADS.len() as u64 - 1) as usize];
            let t2 = PAYLOADS[rng.next_in(0, PAYLOADS.len() as u64 - 1) as usize];
            // r1 -> r2 : T1 . r2 -> r1 : T2 . end
            let g = GlobalType::message(
                r1.as_str(),
                r2.as_str(),
                t1,
                GlobalType::message(r2.as_str(), r1.as_str(), t2, GlobalType::End),
            );
            let proj = g.project_all().expect("linear two-role protocol is realizable");
            let role1 = axon_frontend::multiparty::Role::new(r1);
            let role2 = axon_frontend::multiparty::Role::new(r2);
            let p1 = &proj[&role1];
            let p2 = &proj[&role2];
            assert!(
                p1.is_dual_to(p2),
                "two-role projection breaks the connection law at seed=0x{seed:x}"
            );
        }
    }
}

// Note: the v2.3.0 sealed-runtime round-trip invariant lives in
// `axon-rs/tests/session_type_fuzz.rs` because `SessionRuntime` is in the
// runtime crate (this file's `axon-frontend` cannot reach it).
