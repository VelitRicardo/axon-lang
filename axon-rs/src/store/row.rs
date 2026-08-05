//! §Fase 118.b.3 — the `axonstore` ROW SHAPE and pool sizing, driver-free.
//!
//! The fifth instance of the §118 smell was not one type but a CLUSTER, all
//! parked in `store/postgres_backend.rs` because it was the first module that
//! needed them:
//!
//!   * [`StoreError`](super::error::StoreError) — the error catalog (see
//!     `store/error.rs`), including §113's `LeaseExpired` anchor breach.
//!   * [`StoreRow`] — JSON-safe column/value pairs. `store::epistemic`, which
//!     links no driver, is built on it.
//!   * [`MAX_POOL_CONNECTIONS`] — a number. Its own doc comment records that it
//!     was made `pub` so the registry would use THIS constant "rather than a copy
//!     of the number — a second copy of a fact is how the islands happened".
//!     Gating the driver would have forced that copy back into existence.
//!
//! **This module must never acquire a dependency.**

use serde_json::Value as JsonValue;

/// The legacy pool size — what EVERY `postgresql` axonstore got before §Fase 113,
/// with no environment variable, no config and no source-level knob.
///
/// It survives as the default for a store that names no `resource:` (the soft
/// migration: the live deployment runs on that form). `pub` since §113 so the
/// registry uses THIS constant rather than a copy of the number — a second copy
/// of a fact is how the islands happened.
pub const MAX_POOL_CONNECTIONS: u32 = 10;

/// A single retrieved row, as JSON-safe column → value pairs in column
/// order. Every value is `serde_json`-representable — UUID, TIMESTAMPTZ
/// and NUMERIC are pre-mapped to strings, so an adopter never has to
/// monkey-patch a JSON encoder (the kivi-reported Python pain).
#[derive(Debug, Clone, PartialEq)]
pub struct StoreRow {
    /// Column name → JSON value, in `SELECT` column order.
    pub columns: Vec<(String, JsonValue)>,
}

impl StoreRow {
    /// Look up a column's value by name.
    pub fn get(&self, column: &str) -> Option<&JsonValue> {
        self.columns
            .iter()
            .find(|(name, _)| name == column)
            .map(|(_, value)| value)
    }

    /// Render the row as a JSON object.
    pub fn to_json(&self) -> JsonValue {
        JsonValue::Object(self.columns.iter().cloned().collect())
    }
}
