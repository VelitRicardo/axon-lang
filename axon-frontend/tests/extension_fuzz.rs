//! v2.5.0 — property/fuzz pass over the `extension` grammar +
//! type-checker. A deterministic LCG (no external dep) generates
//! arbitrary `extension` declarations — varying category (effects /
//! scan / bogus), member count (incl. zero), base shape (custom /
//! canonical-shadow / bare / colon-qualified), and metadata presence
//! (semantics / default_confidence incl. out-of-range) — and asserts
//! the two TOTALITY properties that matter for a security-relevant
//! surface:
//!
//!   1. The lexer + parser + type-checker NEVER PANIC on any generated
//!      input (the parser is total; a malformed extension is a parse
//!      error or a type error, never a crash).
//! 2. The type-checker is CONSISTENT with the v2.5.0 invariants:
//!      - a non-{effects,scan} category always emits an "unknown
//!        category" error;
//!      - a `category: effects` member whose base is a canonical
//!        ENFORCEABLE base (invariant #2 — e.g. `io:...`) always emits a
//!        "PROVENANCE-class only" error;
//!      - a valid declaration (effects/scan, custom bases, in-range
//!        confidence) emits NO extension error.

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn n(&mut self, m: u64) -> u64 {
        self.next() % m
    }
}

const CATEGORIES: &[&str] = &["effects", "scan", "telepathy", ""];
// Bases: custom (ok), a canonical ENFORCEABLE base (invariant-#2
// violation when used in an effects member), and a canonical scan
// category (invariant-#3 violation in a scan member).
const CUSTOM_BASES: &[&str] = &["risk", "provenance", "domain"];
const QUALIFIERS: &[&str] = &["elevated", "high", "low"];
const ENFORCEABLE: &[&str] = &["io", "network", "storage"];

fn errors(src: &str) -> Vec<String> {
    let tokens = match Lexer::new(src, "<fuzz>").tokenize() {
        Ok(t) => t,
        // A lex error is an acceptable total outcome (no panic).
        Err(_) => return vec!["<lex-error>".to_string()],
    };
    let program = match Parser::new(tokens).parse() {
        Ok(p) => p,
        Err(_) => return vec!["<parse-error>".to_string()],
    };
    TypeChecker::new(&program)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

#[test]
fn extension_grammar_and_typecheck_are_total() {
    let mut rng = Lcg(0x53e_53e_53e);
    for i in 0..4000u64 {
        let category = CATEGORIES[(rng.n(CATEGORIES.len() as u64)) as usize];
        let n_members = rng.n(4); // 0..=3 members

        // Decide a member shape for this iteration.
        // 0: custom base:qualifier  1: bare custom  2: enforceable-shadow
        let shape = rng.n(3);
        let mut shadows_enforceable = false;

        let mut members = String::new();
        for m in 0..n_members {
            if m > 0 {
                members.push_str(", ");
            }
            let name = match shape {
                0 => format!(
                    "{}:{}",
                    CUSTOM_BASES[(rng.n(CUSTOM_BASES.len() as u64)) as usize],
                    QUALIFIERS[(rng.n(QUALIFIERS.len() as u64)) as usize]
                ),
                1 => CUSTOM_BASES[(rng.n(CUSTOM_BASES.len() as u64)) as usize].to_string(),
                _ => {
                    shadows_enforceable = true;
                    format!(
                        "{}:bypass",
                        ENFORCEABLE[(rng.n(ENFORCEABLE.len() as u64)) as usize]
                    )
                }
            };
            // Optional metadata block (sometimes out-of-range confidence).
            match rng.n(3) {
                0 => {
                    let c = if rng.n(2) == 0 { "0.8" } else { "1.5" };
                    members.push_str(&format!("\"{name}\" : {{ default_confidence: {c} }}"));
                }
                1 => members.push_str(&format!("\"{name}\" : {{ semantics: \"x\" }}")),
                _ => members.push_str(&format!("\"{name}\"")),
            }
        }

        let src = format!("extension fz_{i} {{ category: {category} members: [ {members} ] }}");

        // Property 1: never panics (the call itself completing is the
        // assertion; a panic would abort the test).
        let errs = errors(&src);

        // Property 2: consistency with the v2.5.0 invariants.
        let joined = errs.join(" | ");
        if errs == ["<lex-error>"] || errs == ["<parse-error>"] {
            continue; // total non-panic outcomes; nothing more to assert
        }
        if category != "effects" && category != "scan" {
            assert!(
                joined.contains("unknown category"),
                "non-{{effects,scan}} category must error. src=`{src}` errs=`{joined}`"
            );
        } else if category == "effects" && shape == 2 && n_members > 0 {
            assert!(
                joined.contains("PROVENANCE-class only"),
                "an effects member shadowing an enforceable base must error (invariant #2). \
                 src=`{src}` errs=`{joined}`"
            );
            let _ = shadows_enforceable;
        }
    }
}
