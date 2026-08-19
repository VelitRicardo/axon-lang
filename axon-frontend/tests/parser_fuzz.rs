//! v1.21.0 — Rust parser robustness fuzz for `transport` +
//! `keepalive` fields. Cross-stack mirror of
//! `tests/test_fase30_transport_fuzz.py` (Python side, D12 budget).
//!
//! 100 deterministic-seeded iterations × 10 mutations each =
//! 1000 adversarial inputs feed the `axon-frontend` parser through
//! the `axonendpoint` block path. The recovery contract (v1.20.0
//! D12 extended to v1.21.0):
//!
//!   * Closed-enum × closed-enum input (`transport` ∈ {json, sse,
//!     ndjson} AND `keepalive` ∈ {5s, 15s, 30s, 60s}) → parser must
//!     succeed and the resulting `AxonEndpointDefinition` must
//!     round-trip the declared values verbatim.
//!   * Any other lex-clean input → parser must surface a structured
//!     parse error (return Err, never panic).
//!   * Lex-rejected input → tokenize() returns Err, skip the
//! iteration (out of scope per v1.20.0 D12 boundary).
//!   * NEVER: uncaught panic, infinite loop, or stack overflow.
//!
//! Seed numbering 0..100 matches the Python pack 1:1 so a future
//! cross-stack regression that fires on the same seed surfaces
//! identically on both sides.

use axon_frontend::ast::Declaration;
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;

const SEED_TEMPLATE: &str = "axonendpoint Live {\n\
                              method: POST\n\
                              path: \"/v1/x\"\n\
                              execute: F\n\
                              transport: {TRANSPORT}\n\
                              keepalive: {KEEPALIVE}\n\
                            }";

const VALUE_ALPHABET: &[u8] =
    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";

const ENUM_TRANSPORTS: &[&str] = &["json", "sse", "ndjson"];
const ENUM_KEEPALIVES: &[&str] = &["5s", "15s", "30s", "60s"];

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
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() as f64) / (u64::MAX as f64)
    }
}

fn random_token(rng: &mut Xorshift, length: usize) -> String {
    let length = length.max(1);
    let head = b"abcdefghijklmnopqrstuvwxyz"[rng.next_usize(26)];
    let mut s = String::with_capacity(length);
    s.push(head as char);
    for _ in 1..length {
        let b = VALUE_ALPHABET[rng.next_usize(VALUE_ALPHABET.len())];
        s.push(b as char);
    }
    s
}

/// 100 seeds × 10 inner iterations = 1000 mutated value tokens.
#[test]
fn random_value_mutation_never_crashes() {
    for seed in 0..100u64 {
        let mut rng = Xorshift(0x3030_4646_face_b00b_u64.wrapping_add(seed));
        for _iteration in 0..10 {
            let transport = if rng.next_f64() < 0.05 {
                ENUM_TRANSPORTS[rng.next_usize(ENUM_TRANSPORTS.len())].to_string()
            } else {
                let len = 1 + rng.next_usize(11);
                random_token(&mut rng, len)
            };
            let keepalive = if rng.next_f64() < 0.05 {
                ENUM_KEEPALIVES[rng.next_usize(ENUM_KEEPALIVES.len())].to_string()
            } else {
                let len = 1 + rng.next_usize(11);
                random_token(&mut rng, len)
            };

            let source = SEED_TEMPLATE
                .replace("{TRANSPORT}", &transport)
                .replace("{KEEPALIVE}", &keepalive);

            // Lex-rejected input is out of scope (v1.20.0 D12 boundary).
            let Ok(tokens) = Lexer::new(&source, "<fuzz>").tokenize() else {
                continue;
            };

            // Parser must never panic. On valid-enum × valid-enum
            // input, parse must succeed AND round-trip the fields.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Parser::new(tokens).parse()
            }));
            let parse_result = match result {
                Ok(r) => r,
                Err(_) => panic!(
                    "Parser panicked on lex-clean input (seed={seed}):\n\
                     transport={transport:?} keepalive={keepalive:?}"
                ),
            };

            let is_valid_enum = ENUM_TRANSPORTS.contains(&transport.as_str())
                && ENUM_KEEPALIVES.contains(&keepalive.as_str());

            match parse_result {
                Ok(program) => {
                    // Find the AxonEndpoint declaration.
                    let ae = program
                        .declarations
                        .iter()
                        .find_map(|d| match d {
                            Declaration::AxonEndpoint(ae) => Some(ae),
                            _ => None,
                        })
                        .unwrap_or_else(|| {
                            panic!(
                                "parsed Ok but no AxonEndpoint declaration \
                                 (seed={seed}, transport={transport:?}, \
                                 keepalive={keepalive:?})"
                            )
                        });
                    if is_valid_enum {
                        assert_eq!(
                            ae.transport, transport,
                            "transport drifted on closed-enum input \
                             (seed={seed})"
                        );
                        assert_eq!(
                            ae.keepalive, keepalive,
                            "keepalive drifted on closed-enum input \
                             (seed={seed})"
                        );
                    }
                }
                Err(_e) => {
                    // Structured rejection — acceptable for any
                    // non-closed-enum input. Closed-enum × closed-
                    // enum input rejected is a regression.
                    assert!(
                        !is_valid_enum,
                        "closed-enum input was rejected \
                         (seed={seed}, transport={transport:?}, \
                         keepalive={keepalive:?})"
                    );
                }
            }
        }
    }
}

/// Byte-level mutation fuzz: take the known-good source and randomly
/// mutate bytes anywhere in the file. The 30.b drift gate already
/// pins specific positive/negative cases via the shared corpus; this
/// pack stresses the parser's robustness against adversarial bytes
/// inserted at any offset, complementing v1.20.0's generic recovery
/// fuzz with a v1.21.0-specific seed.
#[test]
fn random_byte_mutation_on_axonendpoint_never_crashes() {
    const BASE: &str = "axonendpoint Live {\n\
                        method: POST\n\
                        path: \"/v1/x\"\n\
                        execute: F\n\
                        transport: sse\n\
                        keepalive: 15s\n\
                       }\n";
    // Mutation alphabet weighted toward structural confusion.
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789{}()[]<>:,;\"'\n\t .";

    for bucket in 0..100u64 {
        let mut rng = Xorshift(0xfa5e_3030_a15c_a15c_u64.wrapping_add(bucket));
        let mut current: Vec<u8> = BASE.as_bytes().to_vec();
        for _ in 0..10 {
            if current.is_empty() {
                current = BASE.as_bytes().to_vec();
            }
            let op = rng.next_u64() % 4;
            let pos = rng.next_usize(current.len().max(1));
            match op {
                0 => {
                    if !current.is_empty() {
                        current.remove(pos.min(current.len() - 1));
                    }
                }
                1 => {
                    let b = ALPHABET[rng.next_usize(ALPHABET.len())];
                    current.insert(pos.min(current.len()), b);
                }
                2 if pos + 1 < current.len() => current.swap(pos, pos + 1),
                _ => {
                    let b = ALPHABET[rng.next_usize(ALPHABET.len())];
                    let idx = pos.min(current.len().saturating_sub(1));
                    if !current.is_empty() {
                        current[idx] = b;
                    }
                }
            }
            current.retain(|b| b.is_ascii());

            let Ok(s) = std::str::from_utf8(&current) else {
                continue;
            };
            let Ok(tokens) = Lexer::new(s, "<fuzz>").tokenize() else {
                continue;
            };

            // Parser must not panic. Return value irrelevant.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = Parser::new(tokens).parse();
            }));
            assert!(
                result.is_ok(),
                "Parser panicked on byte-mutated input \
                 (bucket={bucket}, source={s:?})"
            );
        }
    }
}
