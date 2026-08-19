//! AXON Epistemic Lattice — type subsumption, join, meet, uncertainty propagation.
//!
//! Direct port of axon/compiler/type_checker.py (EpistemicLattice class).
//!
//! Hierarchy:
//!   HighConfidenceFact ≤ CitedFact ≤ FactualClaim ≤ Any
//!   Opinion ≤ Any
//!   Speculation ≤ Any
//!   Uncertainty ≤ Any  (taints everything)
//!   Never ≤ everything  (bottom)

use std::collections::{HashMap, HashSet};

// ── Builtin type sets ───────────────────────────────────────────────────────

pub const EPISTEMIC_TYPES: &[&str] = &["FactualClaim", "Opinion", "Speculation", "Uncertainty"];

pub const CONTENT_TYPES: &[&str] = &["Chunk", "Document", "EntityMap", "Summary", "Translation"];

pub const ANALYSIS_TYPES: &[&str] = &[
    "ConfidenceScore",
    "Contradiction",
    "ReasoningChain",
    "RiskScore",
    "SentimentScore",
];

pub const PRIMITIVE_TYPES: &[&str] = &[
    "Boolean",
    "Duration",
    "Float",
    "Integer",
    // v2.26.0 — `Json` is a first-class, open, semi-structured value
    // type: the recursive sum `null | bool | number | string | [Json] |
    // {string→Json}`, navigated TOTALLY by the same `.field`/`[i]`
    // operators v2.26.0 gave the rigid types (a miss is null-as-a-value,
    // never a panic). It is recognized here so a bare `Json` annotation
    // resolves like any builtin; the optional `Json<T>` shape LENS is a
    // compile-time expectation checked separately (see `check_json_lenses`
    // in the type-checker), never an enforced runtime guarantee
    // (doctrine `open_data_is_total`).
    "Json",
    "List",
    "String",
    "StructuredReport",
];

/// All builtin types recognized by the AXON type system.
pub fn builtin_types() -> HashSet<&'static str> {
    let mut s = HashSet::new();
    for t in EPISTEMIC_TYPES {
        s.insert(*t);
    }
    for t in CONTENT_TYPES {
        s.insert(*t);
    }
    for t in ANALYSIS_TYPES {
        s.insert(*t);
    }
    for t in PRIMITIVE_TYPES {
        s.insert(*t);
    }
    // Lattice-internal types
    s.insert("Any");
    s.insert("Never");
    s.insert("HighConfidenceFact");
    s.insert("CitedFact");
    s
}

/// Ranged types with their valid (min, max) bounds.
pub fn ranged_types() -> HashMap<&'static str, (f64, f64)> {
    let mut m = HashMap::new();
    m.insert("RiskScore", (0.0, 1.0));
    m.insert("ConfidenceScore", (0.0, 1.0));
    m.insert("SentimentScore", (-1.0, 1.0));
    m
}

// ── Epistemic Lattice ───────────────────────────────────────────────────────

/// The lattice parent map: child → parent.
/// `None` means the type is a root (no parent).
fn parents() -> HashMap<&'static str, Option<&'static str>> {
    let mut m = HashMap::new();
    m.insert("HighConfidenceFact", Some("CitedFact"));
    m.insert("CitedFact", Some("FactualClaim"));
    m.insert("FactualClaim", Some("Any"));
    m.insert("Opinion", Some("Any"));
    m.insert("Speculation", Some("Any"));
    m.insert("Uncertainty", Some("Any"));
    m.insert("Any", None);
    m.insert("Never", None);
    m
}

/// Ancestors of a type (inclusive), from most specific to most general.
fn ancestors(ty: &str) -> Vec<String> {
    let map = parents();
    let mut chain = vec![ty.to_string()];
    let mut current = ty;
    loop {
        match map.get(current) {
            Some(Some(parent)) => {
                chain.push(parent.to_string());
                current = parent;
            }
            _ => break,
        }
    }
    chain
}

/// Is `t1` a subtype of `t2`? (t1 ≤ t2 in the lattice)
///
/// Special rules:
///   - Never ≤ everything (bottom type)
///   - Everything ≤ Any (top type)
///   - Uncertainty taints: can be passed anywhere
///   - FactualClaim/CitedFact → String coercion
///   - RiskScore/ConfidenceScore/SentimentScore → Float coercion
///   - StructuredReport satisfies any output contract
pub fn is_subtype(t1: &str, t2: &str) -> bool {
    if t1 == t2 {
        return true;
    }
    if t1 == "Never" {
        return true;
    }
    if t2 == "Any" {
        return true;
    }

    // Strip generic params: "List<String>" → "List"
    let t1_base = t1.split('<').next().unwrap_or(t1);
    let t2_base = t2.split('<').next().unwrap_or(t2);

    if t1_base == t2_base {
        return true;
    }

    // Uncertainty taints: can be passed anywhere
    if t1_base == "Uncertainty" {
        return true;
    }

    // Nominal subtyping via lattice ancestry
    if is_nominal_subtype(t1_base, t2_base) {
        return true;
    }

    false
}

fn is_nominal_subtype(t1: &str, t2: &str) -> bool {
    // Check lattice ancestry
    let anc = ancestors(t1);
    if anc.iter().any(|a| a == t2) {
        return true;
    }

    // Special coercions
    // FactualClaim/CitedFact → String
    if t2 == "String" && (t1 == "FactualClaim" || t1 == "CitedFact") {
        return true;
    }
    // Numeric scores → Float
    if t2 == "Float" && (t1 == "RiskScore" || t1 == "ConfidenceScore" || t1 == "SentimentScore") {
        return true;
    }
    // StructuredReport satisfies any output contract
    if t1 == "StructuredReport" {
        return true;
    }

    false
}

/// Join (supremum / ∨) — least upper bound of two types.
/// Implements "Degradación Epistémica": when combining types,
/// the result is the most specific type that subsumes both.
pub fn join(t1: &str, t2: &str) -> String {
    if t1 == t2 {
        return t1.to_string();
    }
    if t1 == "Never" {
        return t2.to_string();
    }
    if t2 == "Never" {
        return t1.to_string();
    }
    if t1 == "Any" || t2 == "Any" {
        return "Any".to_string();
    }

    // Uncertainty taints
    if t1 == "Uncertainty" || t2 == "Uncertainty" {
        return "Uncertainty".to_string();
    }

    // Find lowest common ancestor
    let anc1 = ancestors(t1);
    let anc2: HashSet<String> = ancestors(t2).into_iter().collect();

    for a in &anc1 {
        if anc2.contains(a) {
            return a.clone();
        }
    }

    "Any".to_string()
}

/// Meet (infimum / ∧) — greatest lower bound of two types.
pub fn meet(t1: &str, t2: &str) -> String {
    if t1 == t2 {
        return t1.to_string();
    }
    if is_subtype(t1, t2) {
        return t1.to_string();
    }
    if is_subtype(t2, t1) {
        return t2.to_string();
    }
    "Never".to_string()
}

/// Propagate uncertainty across a collection of types.
/// Returns the join (supremum) of all types — epistemic degradation.
pub fn propagate_uncertainty(types: &[&str]) -> String {
    if types.is_empty() {
        return "Any".to_string();
    }
    let mut result = types[0].to_string();
    for &t in &types[1..] {
        result = join(&result, t);
    }
    result
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtype_reflexive() {
        assert!(is_subtype("FactualClaim", "FactualClaim"));
        assert!(is_subtype("String", "String"));
    }

    #[test]
    fn subtype_lattice_chain() {
        assert!(is_subtype("HighConfidenceFact", "CitedFact"));
        assert!(is_subtype("HighConfidenceFact", "FactualClaim"));
        assert!(is_subtype("HighConfidenceFact", "Any"));
        assert!(is_subtype("CitedFact", "FactualClaim"));
        assert!(is_subtype("FactualClaim", "Any"));
    }

    #[test]
    fn subtype_not_reverse() {
        assert!(!is_subtype("Any", "FactualClaim"));
        assert!(!is_subtype("FactualClaim", "CitedFact"));
    }

    #[test]
    fn subtype_never_is_bottom() {
        assert!(is_subtype("Never", "Any"));
        assert!(is_subtype("Never", "FactualClaim"));
        assert!(is_subtype("Never", "String"));
    }

    #[test]
    fn subtype_any_is_top() {
        assert!(is_subtype("Opinion", "Any"));
        assert!(is_subtype("Speculation", "Any"));
    }

    #[test]
    fn subtype_uncertainty_taints() {
        assert!(is_subtype("Uncertainty", "FactualClaim"));
        assert!(is_subtype("Uncertainty", "String"));
    }

    #[test]
    fn subtype_coercions() {
        assert!(is_subtype("FactualClaim", "String"));
        assert!(is_subtype("CitedFact", "String"));
        assert!(is_subtype("RiskScore", "Float"));
        assert!(is_subtype("ConfidenceScore", "Float"));
        assert!(is_subtype("SentimentScore", "Float"));
        assert!(is_subtype("StructuredReport", "ContractData"));
    }

    #[test]
    fn join_same() {
        assert_eq!(join("FactualClaim", "FactualClaim"), "FactualClaim");
    }

    #[test]
    fn join_lattice() {
        assert_eq!(join("HighConfidenceFact", "CitedFact"), "CitedFact");
        assert_eq!(join("FactualClaim", "Opinion"), "Any");
        assert_eq!(join("CitedFact", "HighConfidenceFact"), "CitedFact");
    }

    #[test]
    fn join_uncertainty_taints() {
        assert_eq!(join("FactualClaim", "Uncertainty"), "Uncertainty");
        assert_eq!(join("Uncertainty", "String"), "Uncertainty");
    }

    #[test]
    fn join_never_neutral() {
        assert_eq!(join("Never", "FactualClaim"), "FactualClaim");
        assert_eq!(join("String", "Never"), "String");
    }

    #[test]
    fn meet_same() {
        assert_eq!(meet("FactualClaim", "FactualClaim"), "FactualClaim");
    }

    #[test]
    fn meet_subtype() {
        assert_eq!(
            meet("HighConfidenceFact", "FactualClaim"),
            "HighConfidenceFact"
        );
        assert_eq!(
            meet("FactualClaim", "HighConfidenceFact"),
            "HighConfidenceFact"
        );
    }

    #[test]
    fn meet_incompatible() {
        assert_eq!(meet("Opinion", "Speculation"), "Never");
    }

    #[test]
    fn propagation_single() {
        assert_eq!(propagate_uncertainty(&["FactualClaim"]), "FactualClaim");
    }

    #[test]
    fn propagation_degrades() {
        assert_eq!(propagate_uncertainty(&["CitedFact", "Opinion"]), "Any");
        assert_eq!(
            propagate_uncertainty(&["HighConfidenceFact", "CitedFact"]),
            "CitedFact"
        );
    }

    #[test]
    fn propagation_uncertainty_taints_all() {
        assert_eq!(
            propagate_uncertainty(&["FactualClaim", "CitedFact", "Uncertainty"]),
            "Uncertainty"
        );
    }

    #[test]
    fn builtin_types_contains_all() {
        let bt = builtin_types();
        assert!(bt.contains("String"));
        assert!(bt.contains("FactualClaim"));
        assert!(bt.contains("RiskScore"));
        assert!(bt.contains("Document"));
        assert!(bt.contains("Any"));
    }
}
