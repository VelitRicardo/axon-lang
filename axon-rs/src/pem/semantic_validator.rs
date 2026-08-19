//! v2.83.0 — the `SemanticValidator`: `e(t)`, the error signal of a `mandate`.
//!
//! # Why this file exists
//!
//! `mandate` publishes a closed-loop controller (README XV, the Cybernetic
//! Refinement Calculus):
//!
//! ```text
//! u(t) = Kp·e(t) + Ki·∫e dt + Kd·de/dt
//! ```
//!
//! Every other part of that controller — the integral, the derivative,
//! anti-windup, the Lyapunov argument `V(e) = ½e²`, the convergence test
//! `∃t ≤ N : |e(t)| < ε` — is **arithmetic on `e(t)`**. So `e(t)` is not one
//! component among several; it is the only place where the guarantee touches
//! reality. If `e(t)` is fabricated, every theorem downstream is decoration.
//!
//! `docs/papers/paper_mandate.md` leaves `e(t)` abstract. The companion paper,
//! `docs/papers/paper_mathematical_prompt_optimization.md`, makes it concrete —
//! and its definition is the right one for a `mandate`, because a mandate *is* a
//! set of constraints:
//!
//! - **section 5.3** frames the problem as a CSP `(X, D, C)`: `X` the output's
//!   variables, `D` their semantic domains, `C` the constraints. Validation is
//!   the question `r ⊨ C`.
//! - **section 9.1** defines the **Constraint Satisfaction Rate**:
//!
//!   ```text
//!   CSR = |{c ∈ C : r ⊨ c}| / |C|
//!   ```
//!
//! This module computes `CSR` and therefore
//!
//! ```text
//! e = 1 − CSR        e ∈ [0, 1],  e = 0 ⟺ every constraint holds
//! ```
//!
//! # The property that makes it trustworthy
//!
//! **No LLM is consulted to compute `e`.** Every predicate here is a total,
//! deterministic function of the response text. That is deliberate: an error
//! signal produced by asking a model "did you comply?" would put the thing being
//! controlled in charge of measuring its own deviation, and the loop would close
//! on nothing. The precedents in this codebase are v2.23.0 (Advantage Witness) and
//! v2.41.0 (`forge`'s novelty, measured by NCD): **a number the system MEASURES, not
//! one it asserts.**
//!
//! # The refusal that keeps it from going vacuous
//!
//! `CSR` is undefined for `|C| = 0`, and the tempting reading — "no constraints
//! violated, so `e = 0`" — is exactly backwards: a mandate with nothing checkable
//! is a mandate that **cannot fail**, which is the vacuous guarantee this project
//! exists to hunt (v2.67.0, v2.67.0's `lease` with no subject). So an empty or
//! wholly-unlowerable constraint set is a **compile-time refusal**, not an
//! `e` of zero. See [`ConstraintSet::new`] and [`ConstraintSet::lower`].
//!
//! # What this file deliberately does NOT do
//!
//! It does not decide *what to do* about a non-zero `e` — no logit bias, no
//! retry, no prompt rewriting. Those are the controller and the actuator, and
//! they live elsewhere. This file answers one question, exactly:
//! **how far is this response from the constraints it was mandated to satisfy?**

use std::fmt;

use regex::Regex;

/// One machine-checkable constraint — a single `c ∈ C` of the CSP `(X, D, C)`.
///
/// This is a **closed catalog**, keyed by what the runtime can actually decide
/// about a response. That is not a simplification to be relaxed later: a free
/// string here would breed an imaginary catalog of constraint kinds that read
/// plausibly and check nothing, which is a failure mode this codebase has now
/// seen four separate times (v2.63.0, v2.67.0 `resource.kind`, v2.69.0 `provider`).
/// A clause that does not fit one of these arms is not silently approximated —
/// it is reported as [`Clause::Uncheckable`] and the mandate refuses.
///
/// Text predicates (`Contains`, `Absent`, `RequiredWith`) match
/// **case-insensitively**, because they describe natural-language obligations
/// where casing is not the obligation. `Matches` is the escape hatch for anyone
/// who needs exact control: it is applied verbatim, and `(?i)` is available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Predicate {
    /// The response must contain `needle`.
    Contains { needle: String },
    /// The response must not contain `needle`.
    Absent { needle: String },
    /// The response must match `pattern`, a regular expression.
    Matches { pattern: String },
    /// The response's word count must lie within `[min, max]` (inclusive; each
    /// bound optional).
    WordCount {
        min: Option<usize>,
        max: Option<usize>,
    },
    /// The response must parse as a JSON value.
    ParsesAsJson,
    /// Conditional obligation: **if** `trigger` appears, **then** `required`
    /// must appear too.
    ///
    /// This arm exists because the shape "no X *without* Y" is the single most
    /// common form a real mandate takes — README XV's own example is *"no
    /// forward-looking statements without safe harbor language"* — and it is
    /// **not** expressible as `Absent` plus `Contains`. `Absent{X}` forbids the
    /// statement outright; `Contains{Y}` demands the disclaimer even when
    /// nothing triggered it. Only the implication says what was meant.
    RequiredWith { trigger: String, required: String },
    /// v2.88.0 — the response must parse as a JSON **object** carrying a
    /// non-null member named `field`.
    ///
    /// # Why this is a seventh predicate and not `Contains`
    ///
    /// This variant exists to lower a declared `type` into constraints, and the
    /// tempting shortcut — `Contains { needle: "\"amount\"" }` — is exactly the
    /// error this arm avoids: it is a SUBSTRING TEST OVER PROSE. It passes on
    /// the sentence *"I could not determine the amount"*, and it passes on a
    /// response that merely mentions the word. A schema is a claim about
    /// STRUCTURE, and structure is not evidenced by the presence of text.
    ///
    /// So the check parses, requires an object at the root, and looks up the
    /// key. A missing key, a null value, a JSON array, a JSON scalar and
    /// unparseable prose are all failures — each with its own reason, because
    /// "the response is not an object" and "the object lacks `amount`" send an
    /// author to different places.
    ///
    /// Adding a member to a CLOSED catalog is a deliberate act
    /// (`feedback_free_string_field_breeds_fake_catalog`): the six variants
    /// above are all predicates over TEXT, and none of them can express a
    /// structural obligation without lying about what it measured.
    JsonField { field: String },
}

impl Predicate {
    /// Decide this predicate against a response. Total: never panics, never
    /// blocks, never calls out.
    ///
    /// `lower` is the response already lower-cased, passed in so a set of `n`
    /// predicates lower-cases the response once rather than `n` times.
    fn holds(&self, response: &str, lower: &str) -> Result<(), String> {
        match self {
            Predicate::Contains { needle } => {
                if lower.contains(&needle.to_lowercase()) {
                    Ok(())
                } else {
                    Err(format!("the response does not contain {needle:?}"))
                }
            }
            Predicate::Absent { needle } => {
                if lower.contains(&needle.to_lowercase()) {
                    Err(format!(
                        "the response contains {needle:?}, which is forbidden"
                    ))
                } else {
                    Ok(())
                }
            }
            Predicate::Matches { pattern } => match Regex::new(pattern) {
                Ok(re) => {
                    if re.is_match(response) {
                        Ok(())
                    } else {
                        Err(format!("the response does not match /{pattern}/"))
                    }
                }
                // An invalid pattern counts as VIOLATED, never as satisfied.
                // A constraint that cannot be evaluated must not be scored as
                // if it held — that is how a broken guarantee reports success.
                // The compile-time check in `ConstraintSet::new` rejects such
                // patterns outright, so reaching here means one was built by
                // hand; it still fails closed.
                Err(e) => Err(format!("the constraint's pattern /{pattern}/ is not a valid regular expression: {e}")),
            },
            Predicate::WordCount { min, max } => {
                let n = response.split_whitespace().count();
                if let Some(lo) = min {
                    if n < *lo {
                        return Err(format!("the response has {n} words, fewer than the required minimum of {lo}"));
                    }
                }
                if let Some(hi) = max {
                    if n > *hi {
                        return Err(format!("the response has {n} words, more than the permitted maximum of {hi}"));
                    }
                }
                Ok(())
            }
            Predicate::ParsesAsJson => {
                match serde_json::from_str::<serde_json::Value>(response.trim()) {
                    Ok(_) => Ok(()),
                    Err(e) => Err(format!("the response is not valid JSON: {e}")),
                }
            }
            // v2.88.0 — structural, never substring. Each failure names its
            // OWN cause: "not JSON", "not an object" and "no such member" send
            // an author to three different places, and collapsing them into one
            // message would make the CSR uninterpretable.
            Predicate::JsonField { field } => {
                let value: serde_json::Value = match serde_json::from_str(response.trim()) {
                    Ok(v) => v,
                    Err(e) => {
                        return Err(format!(
                            "the response is not valid JSON, so field {field:?} cannot be \
                             present: {e}"
                        ))
                    }
                };
                let Some(obj) = value.as_object() else {
                    return Err(format!(
                        "the response is valid JSON but not an OBJECT, so it cannot carry \
                         field {field:?}"
                    ));
                };
                match obj.get(field) {
                    None => Err(format!("the response object has no member {field:?}")),
                    // A declared field present as `null` is ABSENT for this
                    // purpose. Counting it as satisfied would let a response
                    // claim every field of a schema while carrying none of the
                    // values — the v2.67.0 shape (substituting the belief for the
                    // evidence) inside the very check that exists to measure it.
                    Some(serde_json::Value::Null) => Err(format!(
                        "the response object carries {field:?} but its value is null"
                    )),
                    Some(_) => Ok(()),
                }
            }
            Predicate::RequiredWith { trigger, required } => {
                if lower.contains(&trigger.to_lowercase())
                    && !lower.contains(&required.to_lowercase())
                {
                    Err(format!(
                        "the response contains {trigger:?} but not the {required:?} it requires"
                    ))
                } else {
                    Ok(())
                }
            }
        }
    }

    /// The human-readable obligation, used when a violation is fed back to the
    /// model as the CSP solver's backtracking step (paper section 5.3, step 3:
    /// *"identificar `c_j` violado → inyectar `c_j` como feedback"*).
    fn obligation(&self) -> String {
        match self {
            Predicate::Contains { needle } => format!("must contain {needle:?}"),
            Predicate::Absent { needle } => format!("must not contain {needle:?}"),
            Predicate::Matches { pattern } => format!("must match /{pattern}/"),
            Predicate::WordCount { min, max } => match (min, max) {
                (Some(lo), Some(hi)) => format!("must be between {lo} and {hi} words"),
                (Some(lo), None) => format!("must be at least {lo} words"),
                (None, Some(hi)) => format!("must be at most {hi} words"),
                (None, None) => "has no word-count bound".to_string(),
            },
            Predicate::ParsesAsJson => "must be valid JSON".to_string(),
            Predicate::JsonField { field } => {
                format!("must be a JSON object carrying a non-null {field:?}")
            }
            Predicate::RequiredWith { trigger, required } => {
                format!("must not contain {trigger:?} without also containing {required:?}")
            }
        }
    }
}

/// A constraint as it appears in `C`: a checkable predicate plus the text it was
/// lowered from.
///
/// The `source` is not decoration. When a constraint is violated it is the
/// clause the model is shown, and a model told *"must not contain 'we expect'
/// without also containing 'safe harbor'"* corrects far more reliably than one
/// told *"constraint 3 failed"*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Constraint {
    /// Stable index-derived identifier, e.g. `c1`. Used in verdicts and audit.
    pub id: String,
    /// The clause of the mandate this came from, verbatim.
    pub source: String,
    /// What is actually decided about the response.
    pub predicate: Predicate,
}

/// A clause of a mandate's constraint text, after lowering.
///
/// The two arms are the whole point. Lowering natural language to decidable
/// predicates succeeds for some clauses and not others, and the difference must
/// survive to the caller — because a mandate that silently drops the clauses it
/// could not lower is a mandate that reports `e = 0` for a response violating
/// the very thing the developer wrote it to forbid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Clause {
    /// Lowered to a decidable predicate; it enters `C` and moves `e`.
    Checkable(Constraint),
    /// Understood as an obligation but **not decidable** by this validator.
    ///
    /// It is retained — it is still injected into the prompt, so it still
    /// steers the model — but it contributes nothing to `e`, and its presence
    /// is what [`ConstraintSet::new`] refuses on. Honest degradation: the
    /// clause is visible as unenforced rather than counted as satisfied.
    Uncheckable(String),
}

/// Why a constraint set could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidatorError {
    /// `|C| = 0`. `CSR` is undefined and the mandate would be unfalsifiable.
    NoCheckableConstraints { clauses: Vec<String> },
    /// A `Matches` predicate whose pattern is not a valid regular expression.
    /// Caught here, at construction, rather than surfacing as a permanent
    /// violation at every step of the control loop.
    InvalidPattern { id: String, pattern: String, detail: String },
}

impl fmt::Display for ValidatorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidatorError::NoCheckableConstraints { clauses } => {
                write!(
                    f,
                    "this mandate has no machine-checkable constraint, so its \
                     satisfaction rate is undefined and no response could ever \
                     violate it"
                )?;
                if !clauses.is_empty() {
                    write!(f, "; the clauses that could not be lowered were: ")?;
                    write!(f, "{}", clauses.join("; "))?;
                }
                Ok(())
            }
            ValidatorError::InvalidPattern { id, pattern, detail } => write!(
                f,
                "constraint {id} declares the pattern /{pattern}/, which is not a \
                 valid regular expression: {detail}"
            ),
        }
    }
}

impl std::error::Error for ValidatorError {}

/// The constraint set `C` of the CSP `(X, D, C)` — non-empty by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstraintSet {
    constraints: Vec<Constraint>,
    unchecked: Vec<String>,
}

impl ConstraintSet {
    /// Build `C` from clauses, refusing the two shapes that would make the
    /// mandate unfalsifiable or unevaluable.
    ///
    /// Returns `Err(NoCheckableConstraints)` when nothing lowered — see the
    /// module docs for why that is a refusal and not `e = 0`.
    pub fn new(clauses: Vec<Clause>) -> Result<Self, ValidatorError> {
        let mut constraints = Vec::new();
        let mut unchecked = Vec::new();
        for clause in clauses {
            match clause {
                Clause::Checkable(c) => {
                    if let Predicate::Matches { pattern } = &c.predicate {
                        if let Err(e) = Regex::new(pattern) {
                            return Err(ValidatorError::InvalidPattern {
                                id: c.id.clone(),
                                pattern: pattern.clone(),
                                detail: e.to_string(),
                            });
                        }
                    }
                    constraints.push(c);
                }
                Clause::Uncheckable(text) => unchecked.push(text),
            }
        }
        if constraints.is_empty() {
            return Err(ValidatorError::NoCheckableConstraints { clauses: unchecked });
        }
        Ok(ConstraintSet {
            constraints,
            unchecked,
        })
    }

    /// `|C|` — the denominator of `CSR`. Always `≥ 1`.
    pub fn len(&self) -> usize {
        self.constraints.len()
    }

    /// Always `false`; present because clippy asks for it next to `len`, and
    /// because saying so out loud documents the invariant.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The clauses that were understood but could not be decided. Non-empty
    /// here means the mandate steers the model on those clauses without
    /// enforcing them, and callers are expected to say so rather than imply
    /// full coverage.
    pub fn unchecked(&self) -> &[String] {
        &self.unchecked
    }

    /// The constraints themselves, in declaration order.
    pub fn constraints(&self) -> &[Constraint] {
        &self.constraints
    }

    /// Decide `r ⊨ C` and report `CSR` and `e = 1 − CSR`.
    ///
    /// This is `e(t)`. Deterministic, total, and independent of any model.
    pub fn evaluate(&self, response: &str) -> Verdict {
        let lower = response.to_lowercase();
        let mut satisfied = Vec::new();
        let mut violated = Vec::new();
        for c in &self.constraints {
            match c.predicate.holds(response, &lower) {
                Ok(()) => satisfied.push(c.id.clone()),
                Err(reason) => violated.push(Violation {
                    id: c.id.clone(),
                    source: c.source.clone(),
                    obligation: c.predicate.obligation(),
                    reason,
                }),
            }
        }
        // |C| ≥ 1 by construction, so this division is total.
        let csr = satisfied.len() as f64 / self.constraints.len() as f64;
        Verdict {
            csr,
            error: 1.0 - csr,
            satisfied,
            violated,
        }
    }

    /// Lower a mandate's `constraint:` text into clauses.
    ///
    /// # The controlled subset
    ///
    /// The text is split into clauses on `;` and on `, and `, then each clause
    /// is matched against a **closed set of recognised forms**. Everything else
    /// becomes [`Clause::Uncheckable`].
    ///
    /// This is not natural-language understanding and does not pretend to be.
    /// Claiming to lower arbitrary English into decidable predicates would be
    /// precisely the fabricated capability that `e = 1 − CSR` exists to avoid;
    /// the honest behaviour is a small, documented grammar that either matches
    /// or admits it did not.
    pub fn lower(text: &str) -> Vec<Clause> {
        let mut out = Vec::new();
        for (i, raw) in split_clauses(text).into_iter().enumerate() {
            let clause = raw.trim();
            if clause.is_empty() {
                continue;
            }
            let id = format!("c{}", i + 1);
            match lower_clause(clause) {
                Some(predicate) => out.push(Clause::Checkable(Constraint {
                    id,
                    source: clause.to_string(),
                    predicate,
                })),
                None => out.push(Clause::Uncheckable(clause.to_string())),
            }
        }
        out
    }
}

/// A constraint the response failed, with everything needed to feed it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub id: String,
    pub source: String,
    pub obligation: String,
    pub reason: String,
}

/// The outcome of `r ⊨ C`.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// `CSR = |{c ∈ C: r ⊨ c}| / |C|` — paper section 9.1.
    pub csr: f64,
    /// `e = 1 − CSR ∈ [0, 1]`. The error signal the controller acts on.
    pub error: f64,
    pub satisfied: Vec<String>,
    pub violated: Vec<Violation>,
}

impl Verdict {
    /// Whether every constraint held — `e = 0` exactly.
    pub fn is_satisfied(&self) -> bool {
        self.violated.is_empty()
    }

    /// Whether the response is inside the mandate's convergence band,
    /// `|e| < ε` (README XV's `∃t ≤ N: |e(t)| < ε`).
    pub fn within(&self, epsilon: f64) -> bool {
        self.error.abs() < epsilon
    }

    /// The violated constraints rendered as corrective feedback — the CSP
    /// solver's backtracking step (paper section 5.3).
    pub fn feedback(&self) -> String {
        self.violated
            .iter()
            .map(|v| format!("- {} ({})", v.obligation, v.reason))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Split constraint text into clauses on `;` and on `, and `.
///
/// Plain `,` is deliberately NOT a separator: *"GAAP-compliant financial
/// tables, footnote references"* is one obligation with a list in it, and
/// splitting there would manufacture two half-clauses that lower to nothing.
fn split_clauses(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    for on_semi in text.split(';') {
        let mut rest = on_semi;
        while let Some(idx) = rest.find(", and ") {
            parts.push(rest[..idx].to_string());
            rest = &rest[idx + ", and ".len()..];
        }
        parts.push(rest.to_string());
    }
    parts
}

/// Match one clause against the closed set of recognised forms.
///
/// Returns `None` — never a guess — when the clause is outside the subset.
fn lower_clause(clause: &str) -> Option<Predicate> {
    let c = clause.trim().to_lowercase();
    let c = c
        .trim_start_matches("output ")
        .trim_start_matches("the output ")
        .trim_start_matches("the response ")
        .trim_start_matches("response ")
        .trim();

    // "no X without Y" / "no X unless Y" — the conditional obligation.
    for sep in [" without ", " unless "] {
        if let Some(rest) = c.strip_prefix("no ") {
            if let Some(idx) = rest.find(sep) {
                let trigger = rest[..idx].trim();
                let required = rest[idx + sep.len()..].trim();
                if !trigger.is_empty() && !required.is_empty() {
                    return Some(Predicate::RequiredWith {
                        trigger: trigger.to_string(),
                        required: required.to_string(),
                    });
                }
            }
        }
    }

    // Valid JSON.
    for form in [
        "must be valid json",
        "must be a valid json",
        "must parse as json",
        "must be valid json object",
    ] {
        if c.starts_with(form) {
            return Some(Predicate::ParsesAsJson);
        }
    }

    // Word-count bounds.
    if let Some(n) = trailing_count(&c, "must be at most ", " words") {
        return Some(Predicate::WordCount {
            min: None,
            max: Some(n),
        });
    }
    if let Some(n) = trailing_count(&c, "must be at least ", " words") {
        return Some(Predicate::WordCount {
            min: Some(n),
            max: None,
        });
    }
    if let Some(n) = trailing_count(&c, "must be no more than ", " words") {
        return Some(Predicate::WordCount {
            min: None,
            max: Some(n),
        });
    }

    // Regex.
    if let Some(rest) = c.strip_prefix("must match /") {
        if let Some(pattern) = rest.strip_suffix('/') {
            if !pattern.is_empty() {
                return Some(Predicate::Matches {
                    pattern: pattern.to_string(),
                });
            }
        }
    }

    // Literal presence / absence. Longest prefixes first so "must not contain"
    // is never eaten by "must contain".
    for (prefix, negated) in [
        ("must not contain ", true),
        ("must not include ", true),
        ("must never contain ", true),
        ("must contain ", false),
        ("must include ", false),
    ] {
        if let Some(rest) = c.strip_prefix(prefix) {
            let needle = unquote(rest.trim());
            if !needle.is_empty() {
                return Some(if negated {
                    Predicate::Absent { needle }
                } else {
                    Predicate::Contains { needle }
                });
            }
        }
    }

    None
}

/// `"<prefix>N<suffix>"` → `N`.
fn trailing_count(c: &str, prefix: &str, suffix: &str) -> Option<usize> {
    let rest = c.strip_prefix(prefix)?;
    let digits = rest.strip_suffix(suffix).unwrap_or(rest);
    digits.trim().parse::<usize>().ok()
}

/// Strip a single matched pair of surrounding quotes, if present.
fn unquote(s: &str) -> String {
    let s = s.trim().trim_end_matches('.');
    for q in ['"', '\'', '\u{201c}'] {
        if let Some(rest) = s.strip_prefix(q) {
            let close = if q == '\u{201c}' { '\u{201d}' } else { q };
            if let Some(inner) = rest.strip_suffix(close) {
                return inner.to_string();
            }
        }
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkable(id: &str, predicate: Predicate) -> Clause {
        Clause::Checkable(Constraint {
            id: id.to_string(),
            source: predicate.obligation(),
            predicate,
        })
    }

    fn set(predicates: Vec<Predicate>) -> ConstraintSet {
        ConstraintSet::new(
            predicates
                .into_iter()
                .enumerate()
                .map(|(i, p)| checkable(&format!("c{}", i + 1), p))
                .collect(),
        )
        .expect("fixture must have at least one checkable constraint")
    }

    // ---- e = 1 − CSR, the arithmetic the whole controller stands on ----

    #[test]
    fn csr_is_the_fraction_satisfied_and_error_is_its_complement() {
        let c = set(vec![
            Predicate::Contains {
                needle: "alpha".into(),
            },
            Predicate::Contains {
                needle: "beta".into(),
            },
            Predicate::Contains {
                needle: "gamma".into(),
            },
            Predicate::Contains {
                needle: "delta".into(),
            },
        ]);
        let v = c.evaluate("alpha and beta only");
        assert_eq!(v.csr, 0.5, "2 of 4 constraints hold");
        assert_eq!(v.error, 0.5, "e = 1 − CSR");
        assert_eq!(v.violated.len(), 2);
        assert!(!v.is_satisfied());
    }

    #[test]
    fn full_satisfaction_is_exactly_zero_error() {
        let v = set(vec![
            Predicate::Contains {
                needle: "alpha".into(),
            },
            Predicate::Absent {
                needle: "forbidden".into(),
            },
        ])
        .evaluate("alpha, and nothing else");
        assert_eq!(v.error, 0.0);
        assert!(v.is_satisfied());
        assert!(v.within(1e-9), "zero error is inside every positive band");
    }

    #[test]
    fn total_violation_is_error_one() {
        let v = set(vec![Predicate::Contains {
            needle: "alpha".into(),
        }])
        .evaluate("nothing relevant");
        assert_eq!(v.error, 1.0);
    }

    #[test]
    fn error_is_always_in_the_unit_interval() {
        let c = set(vec![
            Predicate::Contains {
                needle: "a".into(),
            },
            Predicate::Absent {
                needle: "a".into(),
            },
        ]);
        // These two constraints are jointly unsatisfiable; e must still be a
        // real number in [0,1] rather than something the PID cannot integrate.
        for response in ["a", "b", "", "aaaa"] {
            let v = c.evaluate(response);
            assert!(
                (0.0..=1.0).contains(&v.error),
                "e out of range for {response:?}: {}",
                v.error
            );
        }
    }

    // ---- the refusal that keeps the guarantee non-vacuous ----

    #[test]
    fn an_empty_constraint_set_is_refused_not_scored_as_perfect() {
        let err = ConstraintSet::new(vec![]).unwrap_err();
        assert_eq!(
            err,
            ValidatorError::NoCheckableConstraints { clauses: vec![] }
        );
        assert!(
            err.to_string().contains("no response could ever violate it"),
            "the refusal must say WHY, got: {err}"
        );
    }

    #[test]
    fn a_set_of_only_unlowerable_clauses_is_refused() {
        let err = ConstraintSet::new(vec![
            Clause::Uncheckable("output must feel authoritative".into()),
            Clause::Uncheckable("output must be insightful".into()),
        ])
        .unwrap_err();
        match err {
            ValidatorError::NoCheckableConstraints { ref clauses } => {
                assert_eq!(clauses.len(), 2, "the refusal names the clauses it could not lower");
            }
            other => panic!("expected NoCheckableConstraints, got {other:?}"),
        }
    }

    #[test]
    fn one_checkable_clause_among_unlowerable_ones_is_accepted_but_reports_the_gap() {
        let c = ConstraintSet::new(vec![
            Clause::Uncheckable("output must feel authoritative".into()),
            checkable(
                "c2",
                Predicate::Absent {
                    needle: "guarantee".into(),
                },
            ),
        ])
        .expect("one checkable constraint is enough to define CSR");
        assert_eq!(c.len(), 1, "|C| counts ONLY checkable constraints");
        assert_eq!(
            c.unchecked(),
            ["output must feel authoritative"],
            "the unenforced clause stays visible instead of counting as satisfied"
        );
        // The decisive property: the unlowerable clause must not dilute e.
        // This case must use a SATISFIED constraint. With a violated one both
        // 0/1 and 0/2 give e = 1.0, so the assertion would hold even if
        // unchecked clauses were wrongly counted in the denominator — which is
        // exactly what mutation M5 proved before this line existed.
        assert_eq!(
            c.evaluate("no forbidden word here").error,
            0.0,
            "an unenforced clause must not keep e above zero forever; if it              entered |C| the mandate could never converge and would always              exhaust its step budget"
        );
        assert_eq!(c.evaluate("we guarantee results").error, 1.0);
    }

    #[test]
    fn an_invalid_regex_is_refused_at_construction() {
        let err = ConstraintSet::new(vec![checkable(
            "c1",
            Predicate::Matches {
                pattern: "([unclosed".into(),
            },
        )])
        .unwrap_err();
        match err {
            ValidatorError::InvalidPattern { id, .. } => assert_eq!(id, "c1"),
            other => panic!("expected InvalidPattern, got {other:?}"),
        }
    }

    #[test]
    fn an_invalid_regex_reached_at_runtime_counts_as_violated_never_as_satisfied() {
        // Constructed by hand, bypassing `new`'s check — the loop must still
        // fail closed rather than score an unevaluable constraint as holding.
        let p = Predicate::Matches {
            pattern: "([unclosed".into(),
        };
        assert!(p.holds("anything at all", "anything at all").is_err());
    }

    // ---- the predicates themselves ----

    #[test]
    fn text_predicates_ignore_case_but_regex_does_not() {
        let c = set(vec![Predicate::Contains {
            needle: "Safe Harbor".into(),
        }]);
        assert!(c.evaluate("...SAFE HARBOR...").is_satisfied());

        let re = set(vec![Predicate::Matches {
            pattern: "Safe Harbor".into(),
        }]);
        assert!(!re.evaluate("...SAFE HARBOR...").is_satisfied());
        assert!(re.evaluate("...Safe Harbor...").is_satisfied());
    }

    #[test]
    fn word_count_bounds_are_inclusive() {
        let c = set(vec![Predicate::WordCount {
            min: Some(2),
            max: Some(4),
        }]);
        assert!(!c.evaluate("one").is_satisfied());
        assert!(c.evaluate("one two").is_satisfied());
        assert!(c.evaluate("one two three four").is_satisfied());
        assert!(!c.evaluate("one two three four five").is_satisfied());
    }

    #[test]
    fn parses_as_json_tolerates_surrounding_whitespace_only() {
        let c = set(vec![Predicate::ParsesAsJson]);
        assert!(c.evaluate("  {\"a\": 1}\n").is_satisfied());
        assert!(!c.evaluate("Here is your JSON: {\"a\": 1}").is_satisfied());
    }

    #[test]
    fn required_with_is_an_implication_not_a_conjunction() {
        let c = set(vec![Predicate::RequiredWith {
            trigger: "we expect".into(),
            required: "safe harbor".into(),
        }]);
        // Trigger absent ⇒ vacuously satisfied. This is the case that neither
        // Absent nor Contains can express.
        assert!(
            c.evaluate("purely historical results").is_satisfied(),
            "no trigger means no obligation"
        );
        assert!(!c.evaluate("we expect growth").is_satisfied());
        assert!(c
            .evaluate("we expect growth; see safe harbor statement")
            .is_satisfied());
    }

    // ---- lowering: matches or admits it did not ----

    #[test]
    fn the_readme_sec_constraint_lowers_partly_and_says_which_part() {
        let clauses = ConstraintSet::lower(
            "Output must be a valid SEC 10-K section with GAAP-compliant financial \
             tables, footnote references, and no forward-looking statements without \
             safe harbor language",
        );
        let checkable_count = clauses
            .iter()
            .filter(|c| matches!(c, Clause::Checkable(_)))
            .count();
        assert_eq!(
            checkable_count, 1,
            "exactly the 'no X without Y' clause is decidable; the rest is not, \
             and must not be pretended into C"
        );
        let c = ConstraintSet::new(clauses).expect("one checkable clause suffices");
        assert!(
            matches!(
                c.constraints()[0].predicate,
                Predicate::RequiredWith { .. }
            ),
            "got {:?}",
            c.constraints()[0].predicate
        );
        assert_eq!(c.unchecked().len(), 1, "the prose half stays visible");
    }

    #[test]
    fn lowering_recognises_each_form_in_the_closed_subset() {
        let cases: Vec<(&str, Predicate)> = vec![
            (
                "output must contain \"safe harbor\"",
                Predicate::Contains {
                    needle: "safe harbor".into(),
                },
            ),
            (
                "must not contain 'guarantee'",
                Predicate::Absent {
                    needle: "guarantee".into(),
                },
            ),
            (
                "the response must include a citation",
                Predicate::Contains {
                    needle: "a citation".into(),
                },
            ),
            ("output must be valid JSON", Predicate::ParsesAsJson),
            (
                "must be at most 200 words",
                Predicate::WordCount {
                    min: None,
                    max: Some(200),
                },
            ),
            (
                "must be at least 50 words",
                Predicate::WordCount {
                    min: Some(50),
                    max: None,
                },
            ),
            (
                "must match /^SECTION [0-9]+/",
                Predicate::Matches {
                    pattern: "^section [0-9]+".into(),
                },
            ),
            (
                "no projections unless a disclaimer is present",
                Predicate::RequiredWith {
                    trigger: "projections".into(),
                    required: "a disclaimer is present".into(),
                },
            ),
        ];
        for (text, expected) in cases {
            let lowered = ConstraintSet::lower(text);
            assert_eq!(lowered.len(), 1, "one clause expected from {text:?}");
            match &lowered[0] {
                Clause::Checkable(c) => assert_eq!(c.predicate, expected, "for {text:?}"),
                Clause::Uncheckable(t) => panic!("{text:?} should lower, got Uncheckable({t:?})"),
            }
        }
    }

    #[test]
    fn negation_is_never_eaten_by_the_positive_prefix() {
        // "must not contain X" must NOT lower to Contains{"not contain X"}.
        let lowered = ConstraintSet::lower("must not contain \"guarantee\"");
        match &lowered[0] {
            Clause::Checkable(c) => assert_eq!(
                c.predicate,
                Predicate::Absent {
                    needle: "guarantee".into()
                },
                "a negated clause lowered to its own opposite"
            ),
            other => panic!("expected Checkable, got {other:?}"),
        }
    }

    #[test]
    fn prose_that_cannot_be_decided_is_reported_not_guessed() {
        for text in [
            "output must be insightful",
            "the response should feel professional",
            "must be accurate and well-reasoned",
            "",
        ] {
            for clause in ConstraintSet::lower(text) {
                assert!(
                    matches!(clause, Clause::Uncheckable(_)),
                    "{text:?} was guessed into a predicate: {clause:?}"
                );
            }
        }
    }

    #[test]
    fn a_bare_comma_does_not_split_a_single_obligation() {
        // "tables, footnote references" is ONE clause with a list inside it.
        let clauses = ConstraintSet::lower(
            "output must contain \"GAAP tables, footnote references\"",
        );
        assert_eq!(clauses.len(), 1, "a plain comma is not a clause separator");
    }

    #[test]
    fn semicolons_and_and_separate_clauses() {
        let clauses = ConstraintSet::lower(
            "must contain \"alpha\"; must not contain \"beta\", and must be valid JSON",
        );
        assert_eq!(clauses.len(), 3);
        let c = ConstraintSet::new(clauses).unwrap();
        assert_eq!(c.len(), 3);
    }

    // ---- feedback: the CSP solver's backtracking step ----

    #[test]
    fn feedback_names_the_obligation_and_the_reason_for_every_violation() {
        let v = set(vec![
            Predicate::Contains {
                needle: "safe harbor".into(),
            },
            Predicate::Absent {
                needle: "guaranteed".into(),
            },
        ])
        .evaluate("returns are guaranteed");
        let fb = v.feedback();
        assert!(fb.contains("must contain \"safe harbor\""), "{fb}");
        assert!(fb.contains("must not contain \"guaranteed\""), "{fb}");
        assert_eq!(fb.lines().count(), 2);
    }

    #[test]
    fn a_satisfied_verdict_has_nothing_to_feed_back() {
        let v = set(vec![Predicate::Contains {
            needle: "alpha".into(),
        }])
        .evaluate("alpha");
        assert_eq!(v.feedback(), "");
    }

    #[test]
    fn within_is_a_strict_band_matching_the_published_convergence_test() {
        // README XV: `∃t ≤ N: |e(t)| < ε` — strict, so e == ε is NOT inside.
        let v = set(vec![
            Predicate::Contains {
                needle: "a".into(),
            },
            Predicate::Contains {
                needle: "b".into(),
            },
        ])
        .evaluate("a only");
        assert_eq!(v.error, 0.5);
        assert!(!v.within(0.5), "|e| < ε is strict");
        assert!(v.within(0.51));
    }
}
