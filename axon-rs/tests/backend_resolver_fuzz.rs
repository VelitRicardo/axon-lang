//! v1.31.0 (D12) — property / fuzz pass over the Backend
//! Resolution Contract.
//!
//! `resolve_backend` is the heart of v1.31.0 — a pure, total,
//! deterministic resolver. This pack hammers it with deterministic
//! LCG-driven arbitrary inputs and asserts the contract invariants
//! hold over every one:
//!
//!   - **Total** — never panics, always returns `Ok` or
//!     `Err(NoBackendAvailable)` for ANY input.
//!   - **Deterministic** — the same inputs always produce the same
//!     result (called twice, byte-equal).
//!   - **D5 — no silent stub** — the `auto` rungs (registry-ranked,
//!     environment-available) NEVER resolve to `stub`. `stub` is
//!     reachable ONLY by an explicit rung-1/2/3 value.
//!   - **Closed reason catalog** — every `Ok` carries one of the
//!     five documented `reason` slugs.
//!   - **Precedence monotonicity** — an explicit rung-1 value always
//!     wins; resolution honors the published ladder order.
//!
//! Deterministic seed-driven generation (no `rand` dependency) so a
//! CI failure reproduces from the printed seed.

use axon::backend_resolution::{
    is_explicit_backend, resolve_backend, BackendResolutionInputs,
    BackendResolutionReason,
};

/// Tiny LCG — deterministic, reproducible from the seed.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn pick<'a>(&mut self, pool: &[&'a str]) -> &'a str {
        pool[(self.next() % pool.len() as u64) as usize]
    }
    fn opt(&mut self, pool: &[&str]) -> Option<String> {
        // ~25% None, else a pooled value.
        if self.next() % 4 == 0 {
            None
        } else {
            Some(self.pick(pool).to_string())
        }
    }
    fn vec(&mut self, pool: &[&str], max: usize) -> Vec<String> {
        let n = (self.next() % (max as u64 + 1)) as usize;
        (0..n).map(|_| self.pick(pool).to_string()).collect()
    }
}

/// The value pool — concrete providers, the transparent tokens, the
/// no-op, the empty string, and garbage. Deliberately includes
/// `"stub"` so the D5 filter is genuinely exercised on the auto rungs.
const POOL: &[&str] = &[
    "anthropic", "openai", "gemini", "kimi", "glm", "ollama",
    "openrouter", "stub", "auto", "", "  ", "GPT-9", "not_a_backend",
];

const REASON_SLUGS: &[&str] = &[
    "request_explicit", "endpoint_declared", "server_default",
    "registry_ranked", "environment_available",
];

#[test]
fn resolver_fuzz_contract_holds_over_arbitrary_inputs() {
    // 20 000 deterministic iterations across the full input space.
    let mut lcg = Lcg(0x3656_4C00_D12E_5EED);
    for iter in 0..20_000u64 {
        let inputs = BackendResolutionInputs {
            request_backend: lcg.opt(POOL),
            endpoint_backend: lcg.opt(POOL),
            server_default: lcg.opt(POOL),
            registry_ranked: lcg.vec(POOL, 5),
            env_available: lcg.vec(POOL, 5),
        };

        // ── Total + deterministic ──────────────────────────────────
        let a = resolve_backend(&inputs);
        let b = resolve_backend(&inputs);
        assert_eq!(
            a, b,
            "36.l D12: resolve_backend must be deterministic \
             (iter {iter}, inputs {inputs:?})"
        );

        match &a {
            Ok(res) => {
                // ── Closed reason catalog ──────────────────────────
                assert!(
                    REASON_SLUGS.contains(&res.reason.as_slug()),
                    "36.l D12: reason slug '{}' is outside the closed \
                     catalog (iter {iter})",
                    res.reason.as_slug()
                );
                // The resolved backend is never empty.
                assert!(
                    !res.backend.is_empty(),
                    "36.l D12: a resolved backend is never empty \
                     (iter {iter}, inputs {inputs:?})"
                );
                // ── D5 — the auto rungs never land on `stub` ───────
                if matches!(
                    res.reason,
                    BackendResolutionReason::RegistryRanked
                        | BackendResolutionReason::EnvironmentAvailable
                ) {
                    assert_ne!(
                        res.backend, "stub",
                        "36.l D5: auto-resolution (rung {:?}) must NEVER \
                         yield `stub` — iter {iter}, inputs {inputs:?}",
                        res.reason
                    );
                }
                // ── D5 — a `stub` result implies an explicit rung ──
                if res.backend == "stub" {
                    assert!(
                        matches!(
                            res.reason,
                            BackendResolutionReason::RequestExplicit
                                | BackendResolutionReason::EndpointDeclared
                                | BackendResolutionReason::ServerDefault
                        ),
                        "36.l D5: `stub` is reachable ONLY by an explicit \
                         rung-1/2/3 value — iter {iter}, inputs {inputs:?}"
                    );
                }
                // ── Precedence — rung 1 explicit always wins ───────
                if let Some(rb) = &inputs.request_backend {
                    if is_explicit_backend(rb) {
                        assert_eq!(
                            res.reason,
                            BackendResolutionReason::RequestExplicit,
                            "36.l D1: an explicit request backend must \
                             win rung 1 — iter {iter}"
                        );
                        assert_eq!(&res.backend, rb);
                    }
                }
            }
            Err(_) => {
                // Honest failure is legal ONLY when no rung could
                // fire: no explicit rung-1/2/3 value, and no auto-rung
                // entry that is a usable concrete backend (an explicit,
                // non-`stub` name — the resolver skips empty / `auto` /
                // `stub` entries).
                let any_explicit = [
                    &inputs.request_backend,
                    &inputs.endpoint_backend,
                    &inputs.server_default,
                ]
                .iter()
                .filter_map(|s| s.as_deref())
                .any(is_explicit_backend);
                let any_auto = inputs
                    .registry_ranked
                    .iter()
                    .chain(inputs.env_available.iter())
                    .any(|b| is_explicit_backend(b) && b.as_str() != "stub");
                assert!(
                    !any_explicit && !any_auto,
                    "36.l D1: honest failure is legal ONLY when every \
                     rung is empty — iter {iter}, inputs {inputs:?}"
                );
            }
        }
    }
}

#[test]
fn is_explicit_backend_fuzz_total_and_consistent() {
    let mut lcg = Lcg(0xE5C0_FFEE_36_1B);
    for _ in 0..5_000u64 {
        let v = lcg.pick(POOL);
        // Total — never panics; the contract: empty + "auto" are the
        // ONLY transparent values.
        let explicit = is_explicit_backend(v);
        assert_eq!(
            explicit,
            !v.is_empty() && v != "auto",
            "36.l: is_explicit_backend must be transparent for exactly \
             {{\"\", \"auto\"}} — value {v:?}"
        );
    }
}

#[test]
fn honest_failure_message_is_stable_and_actionable() {
    // The Display is reused verbatim as the 36.h wire message — it
    // must name every fix on every call.
    let inputs = BackendResolutionInputs::default();
    let err = resolve_backend(&inputs).expect_err("all rungs empty");
    let msg = err.to_string();
    for needle in [
        "backend:", "ANTHROPIC_API_KEY", "--backend", "stub",
    ] {
        assert!(
            msg.contains(needle),
            "36.l: the honest-failure message must name '{needle}'. \
             Got: {msg}"
        );
    }
}
