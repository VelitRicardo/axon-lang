---
name: dataspace_basic
title: "A `dataspace` — the deterministic data plane's declared store"
summary: "`dataspace` declares an analytical data container: a typed columnar schema (`column <name>: <Type>` over the closed catalog — Text, Int, Float, Bool, Timestamp, Json) instantiated in the deterministic engine at deploy. The schema is a compile-time law (axon-T928): an empty schema, a duplicate column, or an unknown type refuses the program — each type maps 1:1 to a physical buffer layout (validity bitmaps, offset buffers, zone maps). The data-plane verbs operate on it: governed `ingest` (v2.63.0) and the relational query verbs (v2.63.0) are landing; until they do the verbs FAIL CLOSED (`MissingDependency: dataspace_engine`) instead of asking an LLM to narrate data it never loaded. A dataspace is analytical (append-only batches); the transactional store is `axonstore`."
topic: data
primitives:
  - dataspace
  - axonstore
---

// `dataspace` is the ANALYTICAL data container — the deterministic data
// plane's store (v2.63.0). It is the dual of `axonstore`:
//
//   axonstore  = transactional (OLTP): rows, isolation levels, breach policy.
//   dataspace  = analytical   (OLAP): append-only columnar batches, queried
//                with relational algebra (σ/π/γ/⋈), never mutated row-by-row.
//
// The schema is the declaration (v2.63.0): one typed `column` per line,
// over the CLOSED catalog {Text, Int, Float, Bool, Timestamp, Json} —
// each type maps 1:1 to a physical columnar buffer (validity bitmap +
// fixed-width or offset buffers + zone maps). The schema is a
// compile-time law (axon-T928): an empty schema, a duplicate column, or
// an unknown type refuses the program. At deploy, the declaration is
// INSTANTIATED in the deterministic engine — governed `ingest` (v2.63.0)
// appends immutable batches; the relational verbs (v2.63.0) query them.
// Until those land, the data-plane verbs fail CLOSED
// (`MissingDependency: dataspace_engine`) — axon does not ask a
// language model to pretend it loaded your data.

dataspace ClinicalMetrics {
    column region:      Text
    column diagnosis:   Text
    column patient_age: Int
    column cost:        Float
    column admitted_at: Timestamp
}

// The transactional side of the same domain, for contrast: `axonstore`
// is where individual records are written with isolation + breach
// policy. Analytical questions ("average per region", "how many per
// diagnosis") belong to the dataspace, not to row storage.

axonstore PatientLedger {
    backend:     postgresql
    connection:  "postgres://clinical.internal/patient_ledger"
    isolation:   serializable
    on_breach:   raise
    capability:  "clinical.write"
    schema {
        patient_id:  Text primary_key
        diagnosis:   Text not_null
        recorded_at: Timestamp not_null
    }
}
