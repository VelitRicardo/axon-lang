//! v1.30.0 — Pillar I: the epistemic data plane.
//!
//! An `axonstore` is not an opaque byte store — it is a participant in
//! axon's epistemic discipline. This module joins the `axonstore` to
//! the ESK trust model (v1.2.0 / ℰMCP):
//!
//! 1. **Every retrieved tuple is born `Untrusted` (⊥).** A row from
//!    `retrieve from S` is not a fact — it is a *claim*. It enters the
//!    flow at the bottom of the [`EpistemicTaint`] lattice and a
//!    downstream `shield` / `know` / reasoning step must elevate it
//!    before it is trusted. The `retrieve` result is therefore an
//!    **epistemic envelope** ([`retrieve_envelope`]) that carries the
//!    `taint` explicitly — the adopter cannot mistake a claim for a
//!    fact.
//!
//! 2. **`confidence_floor` is enforced at `retrieve`.** A store may
//!    declare `confidence_floor: f`. Each retrieved tuple carries a
//!    stored confidence in the reserved [`CONFIDENCE_COLUMN`] column;
//!    tuples below `f` — and tuples with no stored confidence, which
//!    are at ⊥ — are filtered from the trusted result. The count of
//!    filtered tuples is surfaced, never silently dropped.
//!
//! 3. **`confidence_floor` is enforced at `persist`.** Writing a value
//!    below the floor — or an *un-elevated* value, one carrying no
//!    `_confidence` at all — into a confidence-floored store is a
//!    typed [`EpistemicError`]. You cannot quietly write doubt into a
//!    believed store.
//!
//! # `_confidence` — the reserved column convention
//!
//! v1.30.0 carries no column schema in `IRAxonStore` (D12 — operates
//! against existing tables). A row's stored confidence is therefore
//! read from / written to a reserved column named [`CONFIDENCE_COLUMN`]
//! (`_confidence`). An adopter who declares `confidence_floor` is
//! responsible for their table carrying that column. A row with no
//! `_confidence` value is treated as confidence ⊥ (0.0) — below any
//! positive floor.
//!
//! # OSS / ENTERPRISE seam (section 6 — 35.g is SPLIT)
//!
//! This module is the **OSS mechanism**: rows born `Untrusted`, a
//! numeric `confidence_floor` enforced by a total `≥` comparison. The
//! **enterprise** layer calibrates what a confidence band *means* per
//! regulatory vertical (HIPAA clinical confidence, legal-privilege
//! confidence, fintech AML risk-score confidence). The seam is the
//! plain `f64` floor: enterprise calibration produces the `f64`; this
//! module enforces it unchanged.
//!
//! Pure + total — no I/O. The retrieve/persist composition is wired by
//! the runner (35.e) and the streaming dispatcher (35.f).

use serde_json::Value as JsonValue;
use std::fmt;

use crate::emcp::EpistemicTaint;
use crate::store::filter::SqlValue;
use crate::store::row::StoreRow;

/// The reserved column an `axonstore` row's epistemic confidence is
/// read from (at `retrieve`) and written to (at `persist`). A single
/// leading underscore — distinct from the runtime's `__`-prefixed
/// namespace keys.
pub const CONFIDENCE_COLUMN: &str = "_confidence";

// ════════════════════════════════════════════════════════════════════
//  Confidence extraction
// ════════════════════════════════════════════════════════════════════

/// Parse a textual confidence into a finite `f64`. `None` for a value
/// that is not a finite number.
fn parse_confidence_str(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|f| f.is_finite())
}

/// Extract a confidence from a retrieved column's JSON value. NUMERIC
/// columns arrive as strings (35.c precision-safe mapping), FLOAT8 /
/// INT columns as JSON numbers — both are honored. `None` for any
/// non-numeric value (including JSON `null`).
pub fn confidence_of_json(value: &JsonValue) -> Option<f64> {
    match value {
        JsonValue::Number(n) => n.as_f64().filter(|f| f.is_finite()),
        JsonValue::String(s) => parse_confidence_str(s),
        _ => None,
    }
}

/// Extract a confidence from a to-be-persisted [`SqlValue`]. The
/// runner / dispatcher build persist rows as text bindings, so a
/// `_confidence` binding is normally [`SqlValue::Text`]; numeric
/// variants are honored too. `None` for a non-numeric value.
pub fn confidence_of_sql(value: &SqlValue) -> Option<f64> {
    match value {
        SqlValue::Float(f) => f.is_finite().then_some(*f),
        SqlValue::Integer(n) => Some(*n as f64),
        SqlValue::Text(s) => parse_confidence_str(s),
        SqlValue::Boolean(_) | SqlValue::Null => None,
    }
}

// ════════════════════════════════════════════════════════════════════
//  Retrieved tuple — born Untrusted
// ════════════════════════════════════════════════════════════════════

/// A tuple from `retrieve`, born at the bottom of the epistemic
/// lattice. `taint` is always [`EpistemicTaint::Untrusted`] — a
/// retrieved row is a claim, not a fact, until a reasoning step
/// elevates it. `confidence` is the value of the [`CONFIDENCE_COLUMN`]
/// column (`None` when the column is absent or non-numeric).
#[derive(Debug, Clone, PartialEq)]
pub struct RetrievedRow {
    /// The underlying row data.
    pub row: StoreRow,
    /// The epistemic grade — born `Untrusted`.
    pub taint: EpistemicTaint,
    /// The stored confidence, if the row carries a `_confidence` column.
    pub confidence: Option<f64>,
}

/// Mark every row of a `retrieve` result as born `Untrusted`, reading
/// each row's stored confidence from the reserved [`CONFIDENCE_COLUMN`].
pub fn mark_retrieved(rows: Vec<StoreRow>) -> Vec<RetrievedRow> {
    rows.into_iter()
        .map(|row| {
            let confidence =
                row.get(CONFIDENCE_COLUMN).and_then(confidence_of_json);
            RetrievedRow {
                row,
                taint: EpistemicTaint::Untrusted,
                confidence,
            }
        })
        .collect()
}

// ════════════════════════════════════════════════════════════════════
//  retrieve — confidence_floor enforcement
// ════════════════════════════════════════════════════════════════════

/// The result of applying a store's `confidence_floor` to a retrieved
/// row set: the `trusted` rows (at or above the floor) and the
/// `below_floor` rows (filtered out, but counted — never silently
/// dropped).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FloorOutcome {
    /// Rows whose stored confidence is at or above the floor.
    pub trusted: Vec<RetrievedRow>,
    /// Rows below the floor — filtered from the trusted result.
    pub below_floor: Vec<RetrievedRow>,
}

/// Partition retrieved rows by a store's `confidence_floor`.
///
/// - `floor == None` — the store declares no floor; every row is
///   trusted (still born `Untrusted`, but unfiltered).
/// - `floor == Some(f)` — a row is trusted iff its stored confidence
///   is `>= f`. A row with no stored confidence is at ⊥ (treated as
///   `0.0`) and is below any positive floor.
pub fn enforce_retrieve_floor(
    rows: Vec<RetrievedRow>,
    floor: Option<f64>,
) -> FloorOutcome {
    match floor {
        None => FloorOutcome {
            trusted: rows,
            below_floor: Vec::new(),
        },
        Some(f) => {
            let mut outcome = FloorOutcome::default();
            for r in rows {
                if r.confidence.unwrap_or(0.0) >= f {
                    outcome.trusted.push(r);
                } else {
                    outcome.below_floor.push(r);
                }
            }
            outcome
        }
    }
}

/// Build the **epistemic envelope** for a `retrieve` result — the JSON
/// the runner / dispatcher binds as the step output. The envelope
/// carries the `taint` explicitly so the adopter cannot mistake the
/// retrieved claim for a fact:
///
/// ```json
/// {
///   "taint": "untrusted",
///   "confidence_floor": 0.8,
///   "trusted_rows": 3,
///   "below_floor_filtered": 1,
///   "rows": [ { … }, … ]
/// }
/// ```
pub fn retrieve_envelope(outcome: &FloorOutcome, floor: Option<f64>) -> JsonValue {
    let rows: Vec<JsonValue> =
        outcome.trusted.iter().map(|r| r.row.to_json()).collect();
    serde_json::json!({
        "taint": EpistemicTaint::Untrusted.as_str(),
        "confidence_floor": floor,
        "trusted_rows": outcome.trusted.len(),
        "below_floor_filtered": outcome.below_floor.len(),
        "rows": rows,
    })
}

// ════════════════════════════════════════════════════════════════════
//  persist — confidence_floor enforcement
// ════════════════════════════════════════════════════════════════════

/// A `persist` rejected by a store's `confidence_floor`.
#[derive(Debug, Clone, PartialEq)]
pub enum EpistemicError {
    /// `persist` into a confidence-floored store of a row carrying no
    /// `_confidence` value at all — an un-elevated (⊥) write.
    UnelevatedWrite { store: String, floor: f64 },
    /// `persist` of a row whose `_confidence` is below the floor.
    SubFloorWrite { store: String, confidence: f64, floor: f64 },
    /// The row's `_confidence` value is present but not a number.
    MalformedConfidence { store: String, value: String },
}

impl fmt::Display for EpistemicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EpistemicError::UnelevatedWrite { store, floor } => write!(
                f,
                "persist into axonstore `{store}` blocked: the store \
                 declares confidence_floor {floor} but the row carries \
                 no `{CONFIDENCE_COLUMN}` value — an un-elevated (⊥) \
                 write. Elevate the value before persisting."
            ),
            EpistemicError::SubFloorWrite { store, confidence, floor } => {
                write!(
                    f,
                    "persist into axonstore `{store}` blocked: row \
                     confidence {confidence} is below the store's \
                     confidence_floor {floor}"
                )
            }
            EpistemicError::MalformedConfidence { store, value } => write!(
                f,
                "persist into axonstore `{store}` blocked: the \
                 `{CONFIDENCE_COLUMN}` value `{value}` is not a number"
            ),
        }
    }
}

impl std::error::Error for EpistemicError {}

/// Enforce a store's `confidence_floor` on a to-be-persisted row.
///
/// - `floor == None` — no floor; any write is allowed.
/// - `floor == Some(f)` — the row MUST carry a `_confidence` value
///   that is a number `>= f`. A missing value is an [`EpistemicError::
///   UnelevatedWrite`]; a sub-floor value is a [`EpistemicError::
///   SubFloorWrite`]; a non-numeric value is a [`EpistemicError::
///   MalformedConfidence`].
///
/// Total: every input yields `Ok(())` or a typed error.
pub fn enforce_persist_floor(
    row: &[(String, SqlValue)],
    floor: Option<f64>,
    store_name: &str,
) -> Result<(), EpistemicError> {
    let Some(f) = floor else {
        return Ok(());
    };
    let Some((_, value)) = row.iter().find(|(k, _)| k == CONFIDENCE_COLUMN)
    else {
        return Err(EpistemicError::UnelevatedWrite {
            store: store_name.to_string(),
            floor: f,
        });
    };
    let Some(confidence) = confidence_of_sql(value) else {
        return Err(EpistemicError::MalformedConfidence {
            store: store_name.to_string(),
            value: format!("{value:?}"),
        });
    };
    if confidence < f {
        return Err(EpistemicError::SubFloorWrite {
            store: store_name.to_string(),
            confidence,
            floor: f,
        });
    }
    Ok(())
}

// ════════════════════════════════════════════════════════════════════
//  Unit tests
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pairs: &[(&str, JsonValue)]) -> StoreRow {
        StoreRow {
            columns: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        }
    }

    // ── confidence extraction ────────────────────────────────────────

    #[test]
    fn confidence_of_json_handles_number_and_numeric_string() {
        assert_eq!(confidence_of_json(&serde_json::json!(0.75)), Some(0.75));
        assert_eq!(confidence_of_json(&serde_json::json!(1)), Some(1.0));
        // NUMERIC columns arrive as strings (35.c precision-safe map).
        assert_eq!(
            confidence_of_json(&JsonValue::String("0.9".into())),
            Some(0.9)
        );
        assert_eq!(
            confidence_of_json(&JsonValue::String("  0.42 ".into())),
            Some(0.42)
        );
    }

    #[test]
    fn confidence_of_json_rejects_non_numeric() {
        assert_eq!(confidence_of_json(&JsonValue::Null), None);
        assert_eq!(confidence_of_json(&JsonValue::Bool(true)), None);
        assert_eq!(confidence_of_json(&JsonValue::String("high".into())), None);
    }

    #[test]
    fn confidence_of_sql_handles_each_variant() {
        assert_eq!(confidence_of_sql(&SqlValue::Float(0.6)), Some(0.6));
        assert_eq!(confidence_of_sql(&SqlValue::Integer(1)), Some(1.0));
        assert_eq!(
            confidence_of_sql(&SqlValue::Text("0.85".into())),
            Some(0.85)
        );
        assert_eq!(confidence_of_sql(&SqlValue::Text("nope".into())), None);
        assert_eq!(confidence_of_sql(&SqlValue::Boolean(true)), None);
        assert_eq!(confidence_of_sql(&SqlValue::Null), None);
    }

    // ── mark_retrieved — born Untrusted ──────────────────────────────

    #[test]
    fn every_retrieved_row_is_born_untrusted() {
        let rows = vec![
            row(&[("id", serde_json::json!(1))]),
            row(&[("id", serde_json::json!(2)), ("_confidence", serde_json::json!(0.9))]),
        ];
        let marked = mark_retrieved(rows);
        assert_eq!(marked.len(), 2);
        for r in &marked {
            assert_eq!(r.taint, EpistemicTaint::Untrusted);
        }
        assert_eq!(marked[0].confidence, None);
        assert_eq!(marked[1].confidence, Some(0.9));
    }

    // ── enforce_retrieve_floor ───────────────────────────────────────

    #[test]
    fn no_floor_keeps_every_row_trusted() {
        let marked = mark_retrieved(vec![
            row(&[("id", serde_json::json!(1))]),
            row(&[("id", serde_json::json!(2))]),
        ]);
        let outcome = enforce_retrieve_floor(marked, None);
        assert_eq!(outcome.trusted.len(), 2);
        assert!(outcome.below_floor.is_empty());
    }

    #[test]
    fn floor_partitions_rows_by_stored_confidence() {
        let marked = mark_retrieved(vec![
            row(&[("id", serde_json::json!(1)), ("_confidence", serde_json::json!(0.95))]),
            row(&[("id", serde_json::json!(2)), ("_confidence", serde_json::json!(0.50))]),
            row(&[("id", serde_json::json!(3)), ("_confidence", serde_json::json!(0.80))]),
        ]);
        let outcome = enforce_retrieve_floor(marked, Some(0.8));
        // 0.95 and 0.80 (>= 0.8) trusted; 0.50 below.
        assert_eq!(outcome.trusted.len(), 2);
        assert_eq!(outcome.below_floor.len(), 1);
        assert_eq!(outcome.below_floor[0].confidence, Some(0.50));
    }

    #[test]
    fn a_row_with_no_confidence_is_below_any_positive_floor() {
        let marked = mark_retrieved(vec![row(&[("id", serde_json::json!(1))])]);
        let outcome = enforce_retrieve_floor(marked, Some(0.01));
        assert!(outcome.trusted.is_empty());
        assert_eq!(outcome.below_floor.len(), 1);
    }

    // ── retrieve_envelope ────────────────────────────────────────────

    #[test]
    fn envelope_carries_taint_and_filter_counts() {
        let marked = mark_retrieved(vec![
            row(&[("id", serde_json::json!(1)), ("_confidence", serde_json::json!(0.9))]),
            row(&[("id", serde_json::json!(2)), ("_confidence", serde_json::json!(0.3))]),
        ]);
        let outcome = enforce_retrieve_floor(marked, Some(0.8));
        let env = retrieve_envelope(&outcome, Some(0.8));
        assert_eq!(env["taint"], "untrusted");
        assert_eq!(env["confidence_floor"], 0.8);
        assert_eq!(env["trusted_rows"], 1);
        assert_eq!(env["below_floor_filtered"], 1);
        assert_eq!(env["rows"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn envelope_with_no_floor_has_null_confidence_floor() {
        let outcome = enforce_retrieve_floor(Vec::new(), None);
        let env = retrieve_envelope(&outcome, None);
        assert_eq!(env["taint"], "untrusted");
        assert_eq!(env["confidence_floor"], JsonValue::Null);
        assert_eq!(env["below_floor_filtered"], 0);
    }

    // ── enforce_persist_floor ────────────────────────────────────────

    fn binding(k: &str, v: &str) -> (String, SqlValue) {
        (k.to_string(), SqlValue::Text(v.to_string()))
    }

    #[test]
    fn persist_with_no_floor_is_always_allowed() {
        let row = [binding("name", "Alice")];
        assert!(enforce_persist_floor(&row, None, "s").is_ok());
    }

    #[test]
    fn persist_above_floor_is_allowed() {
        let row = [binding("name", "Alice"), binding("_confidence", "0.9")];
        assert!(enforce_persist_floor(&row, Some(0.8), "tenants").is_ok());
    }

    #[test]
    fn persist_at_the_floor_is_allowed() {
        let row = [binding("_confidence", "0.8")];
        assert!(enforce_persist_floor(&row, Some(0.8), "tenants").is_ok());
    }

    #[test]
    fn persist_below_floor_is_a_subfloor_error() {
        let row = [binding("_confidence", "0.6")];
        match enforce_persist_floor(&row, Some(0.8), "tenants") {
            Err(EpistemicError::SubFloorWrite { confidence, floor, store }) => {
                assert_eq!(confidence, 0.6);
                assert_eq!(floor, 0.8);
                assert_eq!(store, "tenants");
            }
            other => panic!("expected SubFloorWrite, got {other:?}"),
        }
    }

    #[test]
    fn persist_with_no_confidence_into_a_floored_store_is_unelevated() {
        let row = [binding("name", "Alice")];
        assert!(matches!(
            enforce_persist_floor(&row, Some(0.8), "tenants"),
            Err(EpistemicError::UnelevatedWrite { .. })
        ));
    }

    #[test]
    fn persist_with_malformed_confidence_errors() {
        let row = [binding("_confidence", "very-sure")];
        assert!(matches!(
            enforce_persist_floor(&row, Some(0.8), "tenants"),
            Err(EpistemicError::MalformedConfidence { .. })
        ));
    }

    #[test]
    fn every_epistemic_error_has_a_non_empty_display() {
        let errors = [
            EpistemicError::UnelevatedWrite { store: "s".into(), floor: 0.8 },
            EpistemicError::SubFloorWrite {
                store: "s".into(),
                confidence: 0.5,
                floor: 0.8,
            },
            EpistemicError::MalformedConfidence {
                store: "s".into(),
                value: "x".into(),
            },
        ];
        for e in errors {
            assert!(!e.to_string().is_empty());
        }
    }
}
