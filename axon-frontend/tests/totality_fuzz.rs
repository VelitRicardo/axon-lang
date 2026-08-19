//! v1.32.0 (D2, D7) — property/fuzz pass over the compile-time
//! totality check (the v1.32.0 endpoint→flow→parameter coverage
//! gate).
//!
//! Over arbitrary `(flow parameters, body-type fields)` shapes — every
//! combination of covered / uncovered-required / type-mismatched /
//! optional parameters — the type-checker's D2 verdict must EXACTLY
//! equal the totality predicate:
//!
//!     violations = #{ required parameter p of F :
//!                       T has no field named p,
//!                       OR T's field named p has an incompatible type }
//!
//! The check is total — it never panics — and emits exactly one D2
//! error per violated required parameter. A deterministic LCG (no
//! external dep) drives the generator; every iteration cross-checks
//! the type-checker against the predicate computed independently.

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

const NAMES: &[&str] = &["alpha", "beta", "gamma", "delta", "epsilon"];
const TYPES: &[&str] = &["String", "Float"];

/// A generated flow parameter: name, type, and whether it is optional.
#[derive(Clone)]
struct Param {
    name: &'static str,
    ty: &'static str,
    optional: bool,
}

/// A generated body-type field: name + type.
#[derive(Clone)]
struct Field {
    name: &'static str,
    ty: &'static str,
}

/// The D2 totality predicate, computed independently of the
/// type-checker: a REQUIRED parameter is a violation when no
/// same-named field exists, or the same-named field's type differs.
fn expected_violations(params: &[Param], fields: &[Field]) -> usize {
    params
        .iter()
        .filter(|p| {
            if p.optional {
                return false; // an optional parameter is exempt (D2).
            }
            match fields.iter().find(|f| f.name == p.name) {
                None => true,                       // uncovered
                Some(f) => f.ty != p.ty,            // type mismatch
            }
        })
        .count()
}

/// Count the v1.32.0 D2 totality errors a source produces.
fn d2_error_count(src: &str) -> usize {
    let tokens = Lexer::new(src, "<fuzz>").tokenize().expect("lex");
    let prog = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&prog)
        .check()
        .into_iter()
        .filter(|e| e.message.contains("Request Binding Contract"))
        .count()
}

#[test]
fn d2_totality_verdict_matches_the_predicate_over_arbitrary_shapes() {
    let mut lcg = Lcg(0x3737_6700_546F_7461);

    for iter in 0..800u64 {
        // A random UNIQUE subset of names for parameters …
        let mut params: Vec<Param> = Vec::new();
        for &name in NAMES {
            if lcg.n(2) == 0 {
                let ty = TYPES[lcg.n(TYPES.len() as u64) as usize];
                let optional = lcg.n(3) == 0;
                params.push(Param { name, ty, optional });
            }
        }
        // … and an independent random UNIQUE subset for body fields,
        // with at least one field (an empty `type {}` is a separate
        // grammar question, out of this predicate's scope).
        let mut fields: Vec<Field> = Vec::new();
        for &name in NAMES {
            if lcg.n(2) == 0 {
                let ty = TYPES[lcg.n(TYPES.len() as u64) as usize];
                fields.push(Field { name, ty });
            }
        }
        if fields.is_empty() {
            fields.push(Field { name: NAMES[0], ty: TYPES[0] });
        }

        let field_src: String = fields
            .iter()
            .map(|f| format!("{}: {}", f.name, f.ty))
            .collect::<Vec<_>>()
            .join(" ");
        let param_src: String = params
            .iter()
            .map(|p| {
                format!("{}: {}{}", p.name, p.ty, if p.optional { "?" } else { "" })
            })
            .collect::<Vec<_>>()
            .join(", ");

        let src = format!(
            "type FuzzBody {{ {field_src} }}\n\
             flow FuzzFlow({param_src}) -> Unit {{ step S {{ ask: \"x\" output: String }} }}\n\
             axonendpoint FuzzE {{ method: POST path: \"/fz\" \
                 body: FuzzBody execute: FuzzFlow backend: stub }}"
        );

        let got = d2_error_count(&src);
        let expected = expected_violations(&params, &fields);
        assert_eq!(
            got, expected,
            "v1.32.0 D2 — iter {iter}: the type-checker emitted {got} \
             totality error(s); the predicate says {expected}.\n  \
             source:\n{src}"
        );
    }
}
