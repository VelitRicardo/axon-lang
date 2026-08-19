//! v1.22.0 — Rust mirror robustness fuzz for the
//! `compute_implicit_transports` inference pass + warning emission.
//!
//! Mirror of `tests/test_fase31_implicit_fuzz.py` (Python side).
//! Same 100-bucket × 10-iter D12 budget; same seed shapes; same
//! contract:
//!
//!   * Lex+parse-clean adversarial inputs MUST NOT cause either
//!     `compute_implicit_transports` or
//!     `compute_implicit_transport_warnings` to panic.
//!   * After the inference pass, every `AxonEndpointDefinition.
//!     implicit_transport` MUST be `"sse"` or `"json"` (closed
//!     set per D2).
//!   * Re-running the pass on the same program MUST not change
//!     the verdict (idempotence).
//!
//! Pillar trace per D10:
//!   MATHEMATICS — inference is total + deterministic + idempotent.
//!   LOGIC      — output set is closed.
//!   COMPUTING  — `catch_unwind` makes any panic explicit in the
//!                test report; D7 cross-stack parity by sharing
//!                the seed pattern with Python.

use axon_frontend::ast::{Declaration, Program};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::{
    compute_implicit_transport_warnings, compute_implicit_transports,
};

/// Xorshift PRNG — same algorithm as v1.20.0/30 D12 packs. Seed
/// pattern matches Python's random.Random(seed) deterministic shape
/// so cross-stack regressions reproduce on the same bucket.
#[derive(Clone, Copy)]
struct Xorshift(u64);

impl Xorshift {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn next_usize(&mut self, max: usize) -> usize {
        if max == 0 {
            0
        } else {
            (self.next_u64() as usize) % max
        }
    }
}

const SEED_PROGRAMS: &[&str] = &[
    // (a) Kivi-shape — tool with stream effect + apply: in step.
    "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
     flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
     axonendpoint E { method: POST path: \"/f\" execute: F }",
    // (b) Explicit transport: json (D3 opt-out).
    "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
     flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
     axonendpoint E { method: POST path: \"/f\" execute: F transport: json }",
    // (c) Explicit transport: sse.
    "tool t { description: \"t\" effects: <stream:drop_oldest> }\n\
     flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
     axonendpoint E { method: POST path: \"/f\" execute: F transport: sse }",
    // (d) Non-stream flow.
    "flow F() -> Int { step S { ask: \"x\" output: Int } }\n\
     axonendpoint E { method: POST path: \"/f\" execute: F }",
    // (e) Tool with non-stream effect (no inference fires).
    "tool t { description: \"t\" effects: <network> }\n\
     flow F() -> Unit { step S { ask: \"x\" apply: t } }\n\
     axonendpoint E { method: POST path: \"/f\" execute: F }",
];

const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ\
                         0123456789 :{}()[]<>,\"\n_";

/// Apply 0-3 byte mutations: delete / insert / swap / replace.
/// Same shape as Python `_mutate`. Pure ASCII; non-ASCII bytes
/// filtered to keep the lexer happy on most inputs.
fn mutate(source: &[u8], rng: &mut Xorshift) -> Vec<u8> {
    let mut bytes = source.to_vec();
    let n_mut = rng.next_usize(4); // 0..=3
    for _ in 0..n_mut {
        if bytes.is_empty() {
            break;
        }
        let op = rng.next_usize(4);
        let i = rng.next_usize(bytes.len());
        match op {
            0 => {
                bytes.remove(i);
            }
            1 => {
                let b = ALPHABET[rng.next_usize(ALPHABET.len())];
                bytes.insert(i, b);
            }
            2 if i + 1 < bytes.len() => {
                bytes.swap(i, i + 1);
            }
            _ => {
                let b = ALPHABET[rng.next_usize(ALPHABET.len())];
                bytes[i] = b;
            }
        }
    }
    bytes.retain(|b| b.is_ascii());
    bytes
}

fn try_parse(src: &str) -> Option<Program> {
    let tokens = Lexer::new(src, "<fuzz>").tokenize().ok()?;
    Parser::new(tokens).parse().ok()
}

// ─── section 1 — Inference + warning pass never panic ──────────────────────

#[test]
fn inference_pass_never_panics_under_byte_fuzz() {
    let mut total_iter = 0usize;
    let mut exercised = 0usize;
    for bucket in 0..100u64 {
        let mut rng = Xorshift(0x3131_aaff_dead_beef_u64.wrapping_add(bucket));
        for _iter in 0..10 {
            total_iter += 1;
            let base = SEED_PROGRAMS[rng.next_usize(SEED_PROGRAMS.len())];
            let mutated_bytes = mutate(base.as_bytes(), &mut rng);
            let mutated = match std::str::from_utf8(&mutated_bytes) {
                Ok(s) => s.to_string(),
                Err(_) => continue,
            };
            let mut program = match try_parse(&mutated) {
                Some(p) => p,
                None => continue, // out of scope: lex/parse failure
            };
            exercised += 1;
            // `compute_implicit_transports` MUST NOT panic.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                compute_implicit_transports(&mut program);
            }));
            assert!(
                result.is_ok(),
                "compute_implicit_transports panicked (bucket={bucket}, source={mutated:?})"
            );
            // `compute_implicit_transport_warnings` MUST NOT panic.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = compute_implicit_transport_warnings(&program);
            }));
            assert!(
                result.is_ok(),
                "compute_implicit_transport_warnings panicked (bucket={bucket}, source={mutated:?})"
            );
        }
    }
    // Sanity: enough of the 1000 iterations should reach the
    // inference pass (parse-clean). Lower bound 100 — generous.
    assert!(
        exercised >= 100,
        "expected ≥ 100 of {total_iter} fuzz iterations to reach the inference pass, got {exercised}"
    );
}

// ─── section 2 — Output is always in the closed set {"sse", "json"} ───────

#[test]
fn implicit_transport_always_in_closed_set_under_fuzz() {
    for bucket in 0..100u64 {
        let mut rng = Xorshift(0x3131_c105_ed5e_7000_u64.wrapping_add(bucket));
        for _iter in 0..10 {
            let base = SEED_PROGRAMS[rng.next_usize(SEED_PROGRAMS.len())];
            let mutated_bytes = mutate(base.as_bytes(), &mut rng);
            let mutated = match std::str::from_utf8(&mutated_bytes) {
                Ok(s) => s.to_string(),
                Err(_) => continue,
            };
            let mut program = match try_parse(&mutated) {
                Some(p) => p,
                None => continue,
            };
            compute_implicit_transports(&mut program);
            for decl in &program.declarations {
                if let Declaration::AxonEndpoint(ae) = decl {
                    assert!(
                        ae.implicit_transport == "sse" || ae.implicit_transport == "json",
                        "implicit_transport drifted from closed set (bucket={bucket}): \
                         got {:?}, source={mutated:?}",
                        ae.implicit_transport
                    );
                }
            }
        }
    }
}

// ─── section 3 — Idempotence under fuzz ───────────────────────────────────

#[test]
fn idempotence_holds_under_fuzz() {
    // Smaller iteration count — 25 × 10 = 250 mutations.
    for bucket in 0..25u64 {
        let mut rng = Xorshift(0x3131_1de0_e07e_d120_u64.wrapping_add(bucket));
        for _iter in 0..10 {
            let base = SEED_PROGRAMS[rng.next_usize(SEED_PROGRAMS.len())];
            let mutated_bytes = mutate(base.as_bytes(), &mut rng);
            let mutated = match std::str::from_utf8(&mutated_bytes) {
                Ok(s) => s.to_string(),
                Err(_) => continue,
            };
            let mut program = match try_parse(&mutated) {
                Some(p) => p,
                None => continue,
            };

            // Pass 1.
            compute_implicit_transports(&mut program);
            let first: Vec<(String, String)> = program
                .declarations
                .iter()
                .filter_map(|d| match d {
                    Declaration::AxonEndpoint(ae) => {
                        Some((ae.name.clone(), ae.implicit_transport.clone()))
                    }
                    _ => None,
                })
                .collect();
            // Pass 2 — must equal first.
            compute_implicit_transports(&mut program);
            let second: Vec<(String, String)> = program
                .declarations
                .iter()
                .filter_map(|d| match d {
                    Declaration::AxonEndpoint(ae) => {
                        Some((ae.name.clone(), ae.implicit_transport.clone()))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                first, second,
                "idempotence violated (bucket={bucket}, source={mutated:?})"
            );
        }
    }
}
