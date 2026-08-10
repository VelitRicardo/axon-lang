//! §Fase 109.a — the symbolic reverse differentiator over the closed
//! `Expr` (§70) + the deterministic simplifier.
//!
//! # The theorem (differential closure, plan §5.1)
//!
//! Let **D** ⊂ Expr be the differentiable fragment: numeric literals,
//! `Ref`, `Neg`, and `Add/Sub/Mul/Div` over D (plus the `as_float`
//! embedding passthrough). `differentiate` maps D → D — every rule's
//! right-hand side is built from members of D under D's own
//! constructors, so the derivative of a closed expression is *another
//! closed expression*: evaluable by the existing total evaluator,
//! checkable by the existing checker, differentiable again.
//!
//! # The posture
//!
//! Differentiation happens **at compile time, symbolically** — no
//! runtime tape, no finite differences. A non-differentiable construct
//! is a REFUSAL naming the construct and its position inside the
//! expression (`axon-T931`), never a silent zero: a fabricated gradient
//! is the same lie an unstated finite-difference error institutionalizes.
//!
//! # The simplifier (plan §5.3)
//!
//! Deterministic bottom-up rewriting to fixpoint over the identity/
//! absorption rules + literal folding. Terminating (every rule strictly
//! reduces node count or folds to a literal). The simplifier is PART OF
//! THE PROOF CONTRACT (D109.4): the PCC prover and verifier both run it
//! and compare post-simplification — which also keeps the IR gradient
//! human-readable. An inspectable artifact is the point.

use crate::ast::{BinOp, Builtin, Expr, ExprLit, UnOp};

/// A refusal: the expression left the differentiable fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonDifferentiable {
    /// What was found (`"mod"`, `"comparison"`, `"builtin len"`, …).
    pub construct: String,
    /// Where inside the expression (a `.lhs`/`.rhs`/`.arg`-style path
    /// from the root), so the T931 diagnostic can point at it.
    pub path: String,
}

fn lit_f(v: f64) -> Expr {
    Expr::Lit(ExprLit::Float(v))
}

/// ∂e/∂x — plan §5.2. Derivatives are always Float-valued (D109.3:
/// differentiation is over ℝ; Int expressions are embedded).
pub fn differentiate(e: &Expr, wrt: &str) -> Result<Expr, NonDifferentiable> {
    diff_at(e, wrt, String::new())
}

fn diff_at(e: &Expr, wrt: &str, path: String) -> Result<Expr, NonDifferentiable> {
    match e {
        Expr::Lit(ExprLit::Int(_)) | Expr::Lit(ExprLit::Float(_)) => Ok(lit_f(0.0)),
        Expr::Lit(ExprLit::Bool(_)) => Err(NonDifferentiable {
            construct: "bool literal".into(),
            path,
        }),
        Expr::Lit(ExprLit::Str(_)) => Err(NonDifferentiable {
            construct: "string literal".into(),
            path,
        }),
        Expr::Ref(name) => Ok(if name == wrt { lit_f(1.0) } else { lit_f(0.0) }),
        // §Fase 119.o — `let` is REFUSED here, deliberately.
        //
        // Differentiating through a binding needs the chain rule applied to the
        // bound term: ∂/∂w [let x = f in g] = ∂g/∂x · ∂f/∂w + ∂g/∂w. The `Ref`
        // arm above cannot express that — it answers 1 or 0 by NAME, so a
        // reference to `x` inside the body would differentiate to 0 as if `x`
        // were a constant, and the gradient would come back plausible and
        // WRONG. A wrong gradient does not fail; it converges somewhere else.
        //
        // Refusing names the construct and surfaces as axon-T931, which is the
        // §108 posture: an adopter is told, not fooled. Landing it properly
        // means teaching `diff_at` an environment, which is its own decision.
        Expr::Let { .. } => Err(NonDifferentiable {
            construct: "let binding".into(),
            path,
        }),
        Expr::Unary(UnOp::Neg, inner) => Ok(Expr::Unary(
            UnOp::Neg,
            Box::new(diff_at(inner, wrt, format!("{path}.arg"))?),
        )),
        Expr::Unary(UnOp::Not, _) => Err(NonDifferentiable {
            construct: "logical not".into(),
            path,
        }),
        Expr::Binary(op, l, r) => {
            let dl = || diff_at(l, wrt, format!("{path}.lhs"));
            let dr = || diff_at(r, wrt, format!("{path}.rhs"));
            match op {
                BinOp::Add => Ok(Expr::Binary(BinOp::Add, Box::new(dl()?), Box::new(dr()?))),
                BinOp::Sub => Ok(Expr::Binary(BinOp::Sub, Box::new(dl()?), Box::new(dr()?))),
                // Product rule: (e₁·e₂)' = e₁'·e₂ + e₁·e₂'.
                BinOp::Mul => Ok(Expr::Binary(
                    BinOp::Add,
                    Box::new(Expr::Binary(BinOp::Mul, Box::new(dl()?), r.clone())),
                    Box::new(Expr::Binary(BinOp::Mul, l.clone(), Box::new(dr()?))),
                )),
                // Quotient rule: (e₁/e₂)' = (e₁'·e₂ − e₁·e₂') / e₂².
                // `eval_expr` is already total/fail-closed on division —
                // the derivative inherits that honesty (D109.6).
                BinOp::Div => Ok(Expr::Binary(
                    BinOp::Div,
                    Box::new(Expr::Binary(
                        BinOp::Sub,
                        Box::new(Expr::Binary(BinOp::Mul, Box::new(dl()?), r.clone())),
                        Box::new(Expr::Binary(BinOp::Mul, l.clone(), Box::new(dr()?))),
                    )),
                    Box::new(Expr::Binary(BinOp::Mul, r.clone(), r.clone())),
                )),
                // D109.5 — `mod` is non-differentiable at its integer
                // boundaries; an "almost-everywhere" derivative is the
                // hedging we refuse to ship.
                BinOp::Mod => Err(NonDifferentiable {
                    construct: "mod".into(),
                    path,
                }),
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    Err(NonDifferentiable {
                        construct: "comparison".into(),
                        path,
                    })
                }
                BinOp::And | BinOp::Or => Err(NonDifferentiable {
                    construct: "logical connective".into(),
                    path,
                }),
            }
        }
        // The ℝ-embedding passes through untouched.
        Expr::Call(Builtin::AsFloat, args) if args.len() == 1 => {
            diff_at(&args[0], wrt, format!("{path}.arg"))
        }
        Expr::Call(b, _) => Err(NonDifferentiable {
            construct: format!("builtin {}", b.surface()),
            path,
        }),
        Expr::Field(_, name) => Err(NonDifferentiable {
            construct: format!("field access .{name}"),
            path,
        }),
        Expr::Index(_, _) => Err(NonDifferentiable {
            construct: "index access".into(),
            path,
        }),
    }
}

/// Is this literal exactly the given float? (Int literals count: the
/// simplifier treats `0` and `0.0` as the same identity element.)
fn is_lit(e: &Expr, v: f64) -> bool {
    match e {
        Expr::Lit(ExprLit::Float(f)) => *f == v,
        Expr::Lit(ExprLit::Int(i)) => *i as f64 == v,
        _ => false,
    }
}

fn as_num(e: &Expr) -> Option<f64> {
    match e {
        Expr::Lit(ExprLit::Float(f)) => Some(*f),
        Expr::Lit(ExprLit::Int(i)) => Some(*i as f64),
        _ => None,
    }
}

/// Plan §5.3 — deterministic bottom-up simplification to fixpoint.
/// Terminating: every rewrite strictly reduces the node count or folds
/// two literals into one. Confluent on this rule set (no diverging
/// critical pairs). Prover and verifier run the SAME function (D109.4).
pub fn simplify(e: Expr) -> Expr {
    let simplified = simplify_once(e);
    simplified
}

fn simplify_once(e: Expr) -> Expr {
    match e {
        Expr::Unary(UnOp::Neg, inner) => {
            let inner = simplify_once(*inner);
            match as_num(&inner) {
                Some(v) => lit_f(-v),
                None => Expr::Unary(UnOp::Neg, Box::new(inner)),
            }
        }
        Expr::Binary(op, l, r) => {
            let l = simplify_once(*l);
            let r = simplify_once(*r);
            // Literal folding first — two constants become one.
            if let (Some(a), Some(b)) = (as_num(&l), as_num(&r)) {
                let folded = match op {
                    BinOp::Add => Some(a + b),
                    BinOp::Sub => Some(a - b),
                    BinOp::Mul => Some(a * b),
                    BinOp::Div if b != 0.0 => Some(a / b),
                    _ => None,
                };
                if let Some(v) = folded {
                    return lit_f(v);
                }
            }
            match op {
                BinOp::Add if is_lit(&l, 0.0) => r,
                BinOp::Add if is_lit(&r, 0.0) => l,
                BinOp::Sub if is_lit(&r, 0.0) => l,
                BinOp::Mul if is_lit(&l, 0.0) || is_lit(&r, 0.0) => lit_f(0.0),
                BinOp::Mul if is_lit(&l, 1.0) => r,
                BinOp::Mul if is_lit(&r, 1.0) => l,
                BinOp::Div if is_lit(&l, 0.0) => lit_f(0.0),
                BinOp::Div if is_lit(&r, 1.0) => l,
                _ => Expr::Binary(op, Box::new(l), Box::new(r)),
            }
        }
        Expr::Call(b, args) => Expr::Call(b, args.into_iter().map(simplify_once).collect()),
        other => other,
    }
}

/// Convenience: differentiate then simplify — the exact artifact the IR
/// carries and the PCC verifier re-derives.
pub fn grad(e: &Expr, wrt: &str) -> Result<Expr, NonDifferentiable> {
    differentiate(e, wrt).map(simplify)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(name: &str) -> Expr {
        Expr::Ref(name.to_string())
    }
    fn mul(a: Expr, b: Expr) -> Expr {
        Expr::Binary(BinOp::Mul, Box::new(a), Box::new(b))
    }
    fn add(a: Expr, b: Expr) -> Expr {
        Expr::Binary(BinOp::Add, Box::new(a), Box::new(b))
    }

    #[test]
    fn constants_and_refs() {
        assert!(matches!(grad(&lit_f(7.0), "x").unwrap(), Expr::Lit(ExprLit::Float(v)) if v == 0.0));
        assert!(matches!(grad(&r("x"), "x").unwrap(), Expr::Lit(ExprLit::Float(v)) if v == 1.0));
        assert!(matches!(grad(&r("y"), "x").unwrap(), Expr::Lit(ExprLit::Float(v)) if v == 0.0));
    }

    #[test]
    fn product_rule_with_simplification() {
        // d(x·x)/dx = 1·x + x·1 → x + x (the simplifier strips the 1·).
        let e = mul(r("x"), r("x"));
        let d = grad(&e, "x").unwrap();
        match d {
            Expr::Binary(BinOp::Add, l, rr) => {
                assert!(matches!(*l, Expr::Ref(ref n) if n == "x"));
                assert!(matches!(*rr, Expr::Ref(ref n) if n == "x"));
            }
            other => panic!("expected x + x, got {other:?}"),
        }
    }

    #[test]
    fn linear_combination() {
        // d(3·x + y)/dx = 3 ; /dy = 1 — fully folded.
        let e = add(mul(lit_f(3.0), r("x")), r("y"));
        assert!(matches!(grad(&e, "x").unwrap(), Expr::Lit(ExprLit::Float(v)) if v == 3.0));
        assert!(matches!(grad(&e, "y").unwrap(), Expr::Lit(ExprLit::Float(v)) if v == 1.0));
    }

    #[test]
    fn quotient_rule_shape() {
        // d(x/y)/dx = (1·y − x·0)/(y·y) → y / (y·y). (Deliberately NOT
        // cancelled to 1/y: the simplifier does identities + folding,
        // not algebraic cancellation — determinism beats cleverness.)
        let e = Expr::Binary(BinOp::Div, Box::new(r("x")), Box::new(r("y")));
        let d = grad(&e, "x").unwrap();
        match d {
            Expr::Binary(BinOp::Div, num, den) => {
                assert!(matches!(*num, Expr::Ref(ref n) if n == "y"));
                assert!(matches!(*den, Expr::Binary(BinOp::Mul, _, _)));
            }
            other => panic!("expected y/(y·y), got {other:?}"),
        }
    }

    #[test]
    fn chain_through_nesting_and_as_float() {
        // d( as_float((x + 2) · x) )/dx = (x + 2) + x  (after simplify).
        let inner = mul(add(r("x"), lit_f(2.0)), r("x"));
        let e = Expr::Call(Builtin::AsFloat, vec![inner]);
        let d = grad(&e, "x").unwrap();
        assert!(matches!(d, Expr::Binary(BinOp::Add, _, _)), "{d:?}");
    }

    #[test]
    fn refusals_name_construct_and_position() {
        // len(s) — a builtin outside the fragment.
        let e = Expr::Call(Builtin::Length, vec![r("s")]);
        let err = grad(&e, "x").unwrap_err();
        assert!(err.construct.contains("builtin"), "{err:?}");
        // Nested: x + (a % b) — the path points INSIDE.
        let e = add(r("x"), Expr::Binary(BinOp::Mod, Box::new(r("a")), Box::new(r("b"))));
        let err = grad(&e, "x").unwrap_err();
        assert_eq!(err.construct, "mod");
        assert_eq!(err.path, ".rhs");
        // Comparisons + logicals refuse.
        let e = Expr::Binary(BinOp::Lt, Box::new(r("x")), Box::new(lit_f(1.0)));
        assert!(grad(&e, "x").is_err());
    }

    #[test]
    fn differential_closure_grad_of_grad_is_well_defined() {
        // §5.1 corollary: d²(x·x·x)/dx² exists because the first
        // derivative is ITSELF in the fragment.
        let e = mul(mul(r("x"), r("x")), r("x"));
        let d1 = grad(&e, "x").unwrap();
        let d2 = grad(&d1, "x").unwrap();
        // Sanity: evaluate mentally at x=2 → d1 = 3x² = 12; d2 = 6x = 12.
        // Structurally we just require both to exist in-fragment.
        assert!(grad(&d2, "x").is_ok(), "closure holds at every order");
    }

    #[test]
    fn simplifier_is_deterministic_and_idempotent() {
        let e = add(mul(lit_f(0.0), r("x")), mul(r("y"), lit_f(1.0)));
        let s1 = simplify(e.clone());
        let s2 = simplify(s1.clone());
        assert_eq!(format!("{s1:?}"), format!("{s2:?}"), "fixpoint");
        assert!(matches!(s1, Expr::Ref(ref n) if n == "y"));
    }
}
