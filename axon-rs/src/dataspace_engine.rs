//! §Fase 108.b — the deterministic columnar engine behind `dataspace`.
//!
//! The physical store the data-plane verbs operate on (`ingest` §108.c;
//! `focus`/`aggregate`/`associate`/`explore` §108.d). First-party by
//! decision (D108.6-adjacent): the Arrow-class memory *layout* is what
//! buys correctness + scan performance, not the Arrow ecosystem — the
//! same posture as `ooxml.rs` (§99) and `idpe.rs` (§101).
//!
//! # The model (plan §5.1)
//!
//! A dataspace with schema `S = ⟨(name₁,T₁), …, (nameₖ,Tₖ)⟩` holds an
//! ordered sequence of **immutable record batches** (append-only,
//! D108.2 — the analytical dual of `axonstore`'s transactional rows).
//! A batch is `B = ⟨S, {A₁, …, Aₖ}, N, π_B⟩` with `N` the common
//! logical length, `π_B` the provenance stamp, and each column array:
//!
//! - **validity bitmap** `M ∈ {0,1}^⌈N/8⌉` — bit i = element i present.
//!   Nulls are structural; there are NO sentinel values.
//! - **fixed-width buffer** (Int/Float/Timestamp: 8 bytes/element;
//!   Bool: bit-packed) — element i at offset i·w.
//! - **variable-width** (Text/Json): offsets `O ∈ ℤ₊^{N+1}`, monotone,
//!   `O[0] = 0`, `O[N] = |bytes|`; element i = `bytes[O[i]..O[i+1])`.
//!
//! Every invariant is checked ONCE, at construction — a constructed
//! batch is immutable and never re-validated on read.
//!
//! # Zone maps (plan §5.3)
//!
//! Each column of each batch carries `[min, max]` + null-count,
//! computed at construction. §108.d's interval abstraction `φ̂` prunes
//! a batch only when the predicate is provably false over the zone —
//! sound by construction (a skipped batch contains no matching row);
//! completeness is not claimed.
//!
//! # Provenance (plan §5.4)
//!
//! Every batch is stamped at ingest: source + sha256 + declared-time +
//! [`crate::emcp::EpistemicTaint`] (born `Untrusted`, §98 — external
//! data never enters trusted). Query results take the MEET over the
//! batches they touched; no data-plane operation can raise a status.

use std::collections::HashMap;

use crate::emcp::EpistemicTaint;

// ─────────────────────────────────────────────────────────────────────
//  Column types (the D108.1 closed catalog, canonical spellings)
// ─────────────────────────────────────────────────────────────────────

/// The engine-side mirror of the frontend's closed catalog
/// (`DataspaceColumnType`, D108.1). Constructed from the CANONICAL
/// names the IR carries (`visit_dataspace` resolves aliases at IR
/// generation) — an unknown spelling here means a stale or hand-edited
/// artifact, and fails CLOSED.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    Text,
    Int,
    Float,
    Bool,
    Timestamp,
    Json,
}

impl ColumnType {
    pub fn from_canonical(name: &str) -> Option<ColumnType> {
        match name {
            "Text" => Some(ColumnType::Text),
            "Int" => Some(ColumnType::Int),
            "Float" => Some(ColumnType::Float),
            "Bool" => Some(ColumnType::Bool),
            "Timestamp" => Some(ColumnType::Timestamp),
            "Json" => Some(ColumnType::Json),
            _ => None,
        }
    }

    pub fn canonical_name(self) -> &'static str {
        match self {
            ColumnType::Text => "Text",
            ColumnType::Int => "Int",
            ColumnType::Float => "Float",
            ColumnType::Bool => "Bool",
            ColumnType::Timestamp => "Timestamp",
            ColumnType::Json => "Json",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
//  Validity bitmap helpers
// ─────────────────────────────────────────────────────────────────────

#[inline]
fn bitmap_len(n: usize) -> usize {
    n.div_ceil(8)
}

#[inline]
fn bitmap_get(bits: &[u8], i: usize) -> bool {
    (bits[i / 8] >> (i % 8)) & 1 == 1
}

#[inline]
fn bitmap_set(bits: &mut [u8], i: usize) {
    bits[i / 8] |= 1 << (i % 8);
}

// ─────────────────────────────────────────────────────────────────────
//  Column arrays
// ─────────────────────────────────────────────────────────────────────

/// The typed physical buffers of one column. Null slots in fixed-width
/// buffers hold a zero placeholder — NEVER read (the validity bitmap is
/// the single source of presence; there are no sentinel values).
#[derive(Debug, Clone)]
pub enum ColumnData {
    Int(Vec<i64>),
    Float(Vec<f64>),
    /// Bit-packed, `⌈N/8⌉` bytes.
    Bool(Vec<u8>),
    /// Epoch **microseconds** (i64) — one instant encoding, no strings.
    Timestamp(Vec<i64>),
    Text { offsets: Vec<u64>, bytes: Vec<u8> },
    Json { offsets: Vec<u64>, bytes: Vec<u8> },
}

impl ColumnData {
    pub fn column_type(&self) -> ColumnType {
        match self {
            ColumnData::Int(_) => ColumnType::Int,
            ColumnData::Float(_) => ColumnType::Float,
            ColumnData::Bool(_) => ColumnType::Bool,
            ColumnData::Timestamp(_) => ColumnType::Timestamp,
            ColumnData::Text { .. } => ColumnType::Text,
            ColumnData::Json { .. } => ColumnType::Json,
        }
    }
}

/// One immutable column array: validity bitmap + typed buffers.
/// Constructed only through [`ColumnBuilder`] (which enforces the §5.1
/// invariants) or [`ColumnArray::validated`] (which re-checks them).
#[derive(Debug, Clone)]
pub struct ColumnArray {
    len: usize,
    validity: Vec<u8>,
    data: ColumnData,
}

impl ColumnArray {
    /// Construct from raw parts, RE-CHECKING every §5.1 invariant.
    /// The engine's constructors go through [`ColumnBuilder`]; this
    /// exists for deserialization paths (108.e persistence) where the
    /// parts arrive from outside the process boundary.
    pub fn validated(len: usize, validity: Vec<u8>, data: ColumnData) -> Result<Self, String> {
        if validity.len() != bitmap_len(len) {
            return Err(format!(
                "validity bitmap is {} bytes; a column of length {len} requires exactly {}",
                validity.len(),
                bitmap_len(len)
            ));
        }
        match &data {
            ColumnData::Int(v) | ColumnData::Timestamp(v) => {
                if v.len() != len {
                    return Err(format!(
                        "fixed-width buffer holds {} elements; the column declares {len}",
                        v.len()
                    ));
                }
            }
            ColumnData::Float(v) => {
                if v.len() != len {
                    return Err(format!(
                        "fixed-width buffer holds {} elements; the column declares {len}",
                        v.len()
                    ));
                }
            }
            ColumnData::Bool(v) => {
                if v.len() != bitmap_len(len) {
                    return Err(format!(
                        "bit-packed Bool buffer is {} bytes; length {len} requires {}",
                        v.len(),
                        bitmap_len(len)
                    ));
                }
            }
            ColumnData::Text { offsets, bytes } | ColumnData::Json { offsets, bytes } => {
                if offsets.len() != len + 1 {
                    return Err(format!(
                        "offset buffer holds {} entries; a column of length {len} requires {}",
                        offsets.len(),
                        len + 1
                    ));
                }
                if offsets.first() != Some(&0) {
                    return Err("offset buffer must start at 0".to_string());
                }
                if offsets.windows(2).any(|w| w[0] > w[1]) {
                    return Err("offset buffer must be monotone non-decreasing".to_string());
                }
                if offsets.last().copied() != Some(bytes.len() as u64) {
                    return Err(format!(
                        "final offset {} must equal the raw byte buffer length {}",
                        offsets.last().copied().unwrap_or(0),
                        bytes.len()
                    ));
                }
            }
        }
        Ok(ColumnArray {
            len,
            validity,
            data,
        })
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn column_type(&self) -> ColumnType {
        self.data.column_type()
    }

    /// Presence of element `i` — the validity bitmap is the single
    /// source of truth.
    pub fn is_valid(&self, i: usize) -> bool {
        i < self.len && bitmap_get(&self.validity, i)
    }

    pub fn null_count(&self) -> usize {
        (0..self.len).filter(|&i| !self.is_valid(i)).count()
    }

    pub fn get_int(&self, i: usize) -> Option<i64> {
        if !self.is_valid(i) {
            return None;
        }
        match &self.data {
            ColumnData::Int(v) | ColumnData::Timestamp(v) => Some(v[i]),
            _ => None,
        }
    }

    pub fn get_float(&self, i: usize) -> Option<f64> {
        if !self.is_valid(i) {
            return None;
        }
        match &self.data {
            ColumnData::Float(v) => Some(v[i]),
            _ => None,
        }
    }

    pub fn get_bool(&self, i: usize) -> Option<bool> {
        if !self.is_valid(i) {
            return None;
        }
        match &self.data {
            ColumnData::Bool(v) => Some(bitmap_get(v, i)),
            _ => None,
        }
    }

    /// Raw bytes of a variable-width element (`Text` / `Json`).
    pub fn get_bytes(&self, i: usize) -> Option<&[u8]> {
        if !self.is_valid(i) {
            return None;
        }
        match &self.data {
            ColumnData::Text { offsets, bytes } | ColumnData::Json { offsets, bytes } => {
                Some(&bytes[offsets[i] as usize..offsets[i + 1] as usize])
            }
            _ => None,
        }
    }

    pub fn get_text(&self, i: usize) -> Option<&str> {
        self.get_bytes(i)
            .and_then(|b| std::str::from_utf8(b).ok())
    }
}

// ─────────────────────────────────────────────────────────────────────
//  Column builder (ingest-side, §108.c)
// ─────────────────────────────────────────────────────────────────────

/// Append-only builder for one column. Type mismatches REFUSE (D108.7 —
/// silent coercion is data laundering); nullability via [`push_null`]
/// is the only flexibility.
///
/// [`push_null`]: ColumnBuilder::push_null
#[derive(Debug)]
pub struct ColumnBuilder {
    ty: ColumnType,
    len: usize,
    valid: Vec<bool>,
    ints: Vec<i64>,
    floats: Vec<f64>,
    bools: Vec<bool>,
    var_bytes: Vec<u8>,
    var_offsets: Vec<u64>,
}

impl ColumnBuilder {
    pub fn new(ty: ColumnType) -> Self {
        ColumnBuilder {
            ty,
            len: 0,
            valid: Vec::new(),
            ints: Vec::new(),
            floats: Vec::new(),
            bools: Vec::new(),
            var_bytes: Vec::new(),
            var_offsets: vec![0],
        }
    }

    pub fn push_null(&mut self) {
        self.valid.push(false);
        self.len += 1;
        match self.ty {
            ColumnType::Int | ColumnType::Timestamp => self.ints.push(0),
            ColumnType::Float => self.floats.push(0.0),
            ColumnType::Bool => self.bools.push(false),
            ColumnType::Text | ColumnType::Json => {
                self.var_offsets.push(self.var_bytes.len() as u64)
            }
        }
    }

    pub fn push_int(&mut self, v: i64) -> Result<(), String> {
        match self.ty {
            ColumnType::Int | ColumnType::Timestamp => {
                self.valid.push(true);
                self.len += 1;
                self.ints.push(v);
                Ok(())
            }
            other => Err(format!(
                "type mismatch: pushed Int into a {} column",
                other.canonical_name()
            )),
        }
    }

    pub fn push_float(&mut self, v: f64) -> Result<(), String> {
        match self.ty {
            ColumnType::Float => {
                self.valid.push(true);
                self.len += 1;
                self.floats.push(v);
                Ok(())
            }
            other => Err(format!(
                "type mismatch: pushed Float into a {} column",
                other.canonical_name()
            )),
        }
    }

    pub fn push_bool(&mut self, v: bool) -> Result<(), String> {
        match self.ty {
            ColumnType::Bool => {
                self.valid.push(true);
                self.len += 1;
                self.bools.push(v);
                Ok(())
            }
            other => Err(format!(
                "type mismatch: pushed Bool into a {} column",
                other.canonical_name()
            )),
        }
    }

    pub fn push_text(&mut self, v: &str) -> Result<(), String> {
        match self.ty {
            ColumnType::Text => {
                self.valid.push(true);
                self.len += 1;
                self.var_bytes.extend_from_slice(v.as_bytes());
                self.var_offsets.push(self.var_bytes.len() as u64);
                Ok(())
            }
            other => Err(format!(
                "type mismatch: pushed Text into a {} column",
                other.canonical_name()
            )),
        }
    }

    pub fn push_json_bytes(&mut self, v: &[u8]) -> Result<(), String> {
        match self.ty {
            ColumnType::Json => {
                self.valid.push(true);
                self.len += 1;
                self.var_bytes.extend_from_slice(v);
                self.var_offsets.push(self.var_bytes.len() as u64);
                Ok(())
            }
            other => Err(format!(
                "type mismatch: pushed Json into a {} column",
                other.canonical_name()
            )),
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn finish(self) -> ColumnArray {
        let mut validity = vec![0u8; bitmap_len(self.len)];
        for (i, &v) in self.valid.iter().enumerate() {
            if v {
                bitmap_set(&mut validity, i);
            }
        }
        let data = match self.ty {
            ColumnType::Int => ColumnData::Int(self.ints),
            ColumnType::Timestamp => ColumnData::Timestamp(self.ints),
            ColumnType::Float => ColumnData::Float(self.floats),
            ColumnType::Bool => {
                let mut packed = vec![0u8; bitmap_len(self.len)];
                for (i, &b) in self.bools.iter().enumerate() {
                    if b {
                        bitmap_set(&mut packed, i);
                    }
                }
                ColumnData::Bool(packed)
            }
            ColumnType::Text => ColumnData::Text {
                offsets: self.var_offsets,
                bytes: self.var_bytes,
            },
            ColumnType::Json => ColumnData::Json {
                offsets: self.var_offsets,
                bytes: self.var_bytes,
            },
        };
        // The builder maintains the invariants by construction; the
        // validated() pass is kept as the single choke point anyway —
        // cheap, and it means NO ColumnArray exists unchecked.
        ColumnArray::validated(self.len, validity, data)
            .expect("ColumnBuilder maintains the column invariants by construction")
    }
}

// ─────────────────────────────────────────────────────────────────────
//  Zone maps (plan §5.3)
// ─────────────────────────────────────────────────────────────────────

/// Per-column, per-batch statistics — computed once at construction,
/// immutable thereafter. §108.d's interval abstraction reads these to
/// prune batches soundly (skip ⟹ provably no matching row).
#[derive(Debug, Clone, PartialEq)]
pub enum ZoneStats {
    Int { min: i64, max: i64 },
    Float { min: f64, max: f64 },
    Text { min: String, max: String },
    Bool { any_true: bool, any_false: bool },
    /// No valid values in this column of this batch (all null), or a
    /// type without an ordering abstraction (Json).
    None,
}

#[derive(Debug, Clone)]
pub struct ZoneMap {
    pub null_count: usize,
    pub stats: ZoneStats,
}

fn compute_zone_map(col: &ColumnArray) -> ZoneMap {
    let null_count = col.null_count();
    let n = col.len();
    let stats = match col.column_type() {
        ColumnType::Int | ColumnType::Timestamp => {
            let vals: Vec<i64> = (0..n).filter_map(|i| col.get_int(i)).collect();
            match (vals.iter().min(), vals.iter().max()) {
                (Some(&min), Some(&max)) => ZoneStats::Int { min, max },
                _ => ZoneStats::None,
            }
        }
        ColumnType::Float => {
            let vals: Vec<f64> = (0..n).filter_map(|i| col.get_float(i)).collect();
            if vals.is_empty() {
                ZoneStats::None
            } else {
                // f64 min/max via fold — total for non-NaN; a NaN would
                // poison the zone, so its presence degrades to None
                // (scan the batch — conservative, never unsound).
                if vals.iter().any(|v| v.is_nan()) {
                    ZoneStats::None
                } else {
                    let min = vals.iter().copied().fold(f64::INFINITY, f64::min);
                    let max = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    ZoneStats::Float { min, max }
                }
            }
        }
        ColumnType::Bool => {
            let vals: Vec<bool> = (0..n).filter_map(|i| col.get_bool(i)).collect();
            if vals.is_empty() {
                ZoneStats::None
            } else {
                ZoneStats::Bool {
                    any_true: vals.iter().any(|&b| b),
                    any_false: vals.iter().any(|&b| !b),
                }
            }
        }
        ColumnType::Text => {
            let vals: Vec<&str> = (0..n).filter_map(|i| col.get_text(i)).collect();
            match (vals.iter().min(), vals.iter().max()) {
                (Some(min), Some(max)) => ZoneStats::Text {
                    min: min.to_string(),
                    max: max.to_string(),
                },
                _ => ZoneStats::None,
            }
        }
        ColumnType::Json => ZoneStats::None,
    };
    ZoneMap { null_count, stats }
}

// ─────────────────────────────────────────────────────────────────────
//  Provenance (plan §5.4)
// ─────────────────────────────────────────────────────────────────────

/// The stamp every batch carries from ingest. `ingested_at` is the
/// flow's DECLARED time (§91 — one instant per run), not a wall-clock
/// read. `taint` is born [`EpistemicTaint::Untrusted`] (§98) and no
/// data-plane operation can raise it — query results take the meet.
#[derive(Debug, Clone)]
pub struct BatchProvenance {
    pub source: String,
    pub source_sha256: String,
    pub ingested_at: String,
    pub taint: EpistemicTaint,
}

/// The §5.4 meet (⊓) on the taint lattice:
/// `Untrusted < SchemaValidated < Elevated`. An aggregate over any
/// untrusted batch is born untrusted — taint survives the algebra.
pub fn taint_meet(a: EpistemicTaint, b: EpistemicTaint) -> EpistemicTaint {
    fn rank(t: EpistemicTaint) -> u8 {
        match t {
            EpistemicTaint::Untrusted => 0,
            EpistemicTaint::SchemaValidated => 1,
            EpistemicTaint::Elevated => 2,
        }
    }
    if rank(a) <= rank(b) {
        a
    } else {
        b
    }
}

// ─────────────────────────────────────────────────────────────────────
//  Record batches
// ─────────────────────────────────────────────────────────────────────

/// One immutable batch: `B = ⟨S, {A₁…Aₖ}, N, π_B⟩`. Constructed only
/// via [`RecordBatch::new`], which checks schema conformance + the
/// common-length invariant and computes the zone maps ONCE.
#[derive(Debug, Clone)]
pub struct RecordBatch {
    n: usize,
    columns: Vec<ColumnArray>,
    zone_maps: Vec<ZoneMap>,
    provenance: BatchProvenance,
}

impl RecordBatch {
    pub fn new(
        schema: &[(String, ColumnType)],
        columns: Vec<ColumnArray>,
        provenance: BatchProvenance,
    ) -> Result<Self, String> {
        if columns.len() != schema.len() {
            return Err(format!(
                "batch carries {} columns; the schema declares {}",
                columns.len(),
                schema.len()
            ));
        }
        for ((name, ty), col) in schema.iter().zip(&columns) {
            if col.column_type() != *ty {
                return Err(format!(
                    "column `{name}` is declared {} but the batch carries {}",
                    ty.canonical_name(),
                    col.column_type().canonical_name()
                ));
            }
        }
        let n = columns.first().map(|c| c.len()).unwrap_or(0);
        if let Some(bad) = columns.iter().find(|c| c.len() != n) {
            return Err(format!(
                "ragged batch: common logical length is {n}, but a {} column holds {}",
                bad.column_type().canonical_name(),
                bad.len()
            ));
        }
        let zone_maps = columns.iter().map(compute_zone_map).collect();
        Ok(RecordBatch {
            n,
            columns,
            zone_maps,
            provenance,
        })
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    pub fn column(&self, idx: usize) -> Option<&ColumnArray> {
        self.columns.get(idx)
    }

    pub fn zone_map(&self, idx: usize) -> Option<&ZoneMap> {
        self.zone_maps.get(idx)
    }

    pub fn provenance(&self) -> &BatchProvenance {
        &self.provenance
    }
}

// ─────────────────────────────────────────────────────────────────────
//  Stores + the engine
// ─────────────────────────────────────────────────────────────────────

/// One declared dataspace: its schema + the append-only batch list.
#[derive(Debug)]
pub struct DataspaceStore {
    pub name: String,
    schema: Vec<(String, ColumnType)>,
    batches: Vec<RecordBatch>,
}

impl DataspaceStore {
    pub fn schema(&self) -> &[(String, ColumnType)] {
        &self.schema
    }

    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.schema.iter().position(|(n, _)| n == name)
    }

    /// Append an ingested batch (§108.c). Schema conformance was
    /// checked at batch construction against THIS schema; re-checked
    /// here because `RecordBatch::new` and `append` may be fed from
    /// different call sites.
    pub fn append(&mut self, batch: RecordBatch) -> Result<(), String> {
        if batch.columns.len() != self.schema.len() {
            return Err(format!(
                "batch carries {} columns; dataspace `{}` declares {}",
                batch.columns.len(),
                self.name,
                self.schema.len()
            ));
        }
        for ((name, ty), col) in self.schema.iter().zip(&batch.columns) {
            if col.column_type() != *ty {
                return Err(format!(
                    "column `{name}` of dataspace `{}` is {} but the batch carries {}",
                    self.name,
                    ty.canonical_name(),
                    col.column_type().canonical_name()
                ));
            }
        }
        self.batches.push(batch);
        Ok(())
    }

    pub fn batches(&self) -> &[RecordBatch] {
        &self.batches
    }

    /// §108.x — drop ALL ingested batches (whole-dataspace granularity,
    /// D108.2); the declaration + schema persist. Returns the count.
    pub fn clear_batches(&mut self) -> usize {
        let n = self.batches.len();
        self.batches.clear();
        n
    }

    pub fn row_count(&self) -> usize {
        self.batches.iter().map(|b| b.n).sum()
    }

    /// Total resident bytes across all buffers — the quota signal for
    /// the §108.e per-tenant byte budget (refusal, not eviction).
    pub fn resident_bytes(&self) -> usize {
        self.batches
            .iter()
            .flat_map(|b| b.columns.iter())
            .map(|c| {
                c.validity.len()
                    + match &c.data {
                        ColumnData::Int(v) | ColumnData::Timestamp(v) => v.len() * 8,
                        ColumnData::Float(v) => v.len() * 8,
                        ColumnData::Bool(v) => v.len(),
                        ColumnData::Text { offsets, bytes }
                        | ColumnData::Json { offsets, bytes } => offsets.len() * 8 + bytes.len(),
                    }
            })
            .sum()
    }
}

/// The engine: every declared dataspace, instantiated at deploy from
/// the IR (`dataspace_specs`, un-skipped in §108.b). Shared behind
/// `Arc<RwLock<…>>` as the dispatcher port — absent port ⇒ the §108.a
/// handlers fail CLOSED.
#[derive(Debug, Default)]
pub struct DataspaceEngine {
    stores: HashMap<String, DataspaceStore>,
}

/// The dispatcher-port shape (`DispatchCtx.dataspace_engine`).
pub type SharedDataspaceEngine = std::sync::Arc<std::sync::RwLock<DataspaceEngine>>;

impl DataspaceEngine {
    /// Instantiate every declared dataspace from the compiled IR. An
    /// unknown canonical type means a stale or hand-edited artifact —
    /// the whole deploy REFUSES (fail closed), never a partial engine.
    pub fn from_ir(specs: &[axon_frontend::ir_nodes::IRDataspace]) -> Result<Self, String> {
        let mut stores = HashMap::new();
        for spec in specs {
            let mut schema = Vec::with_capacity(spec.columns.len());
            for col in &spec.columns {
                let ty = ColumnType::from_canonical(&col.column_type).ok_or_else(|| {
                    format!(
                        "dataspace `{}` column `{}` carries unknown canonical type `{}` — \
                         the compile-time axon-T928 check did not run over this IR \
                         (stale or hand-edited artifact)",
                        spec.name, col.name, col.column_type
                    )
                })?;
                schema.push((col.name.clone(), ty));
            }
            stores.insert(
                spec.name.clone(),
                DataspaceStore {
                    name: spec.name.clone(),
                    schema,
                    batches: Vec::new(),
                },
            );
        }
        Ok(DataspaceEngine { stores })
    }

    /// Merge a deployed program's declared dataspaces into a live engine
    /// (the OSS server hosts several programs). A NEW name creates an
    /// empty store; a RE-declared name **replaces** its store (the schema
    /// may have changed, and the OSS engine is in-memory — redeploy has
    /// restart-equivalent semantics for that dataspace, D108.8). Names
    /// not in `specs` are untouched. Validation is all-or-nothing: an
    /// unknown canonical type refuses the whole merge, mutating nothing.
    pub fn merge_from_ir(
        &mut self,
        specs: &[axon_frontend::ir_nodes::IRDataspace],
    ) -> Result<(), String> {
        let incoming = DataspaceEngine::from_ir(specs)?;
        for (name, store) in incoming.stores {
            self.stores.insert(name, store);
        }
        Ok(())
    }

    pub fn store(&self, name: &str) -> Option<&DataspaceStore> {
        self.stores.get(name)
    }

    pub fn store_mut(&mut self, name: &str) -> Option<&mut DataspaceStore> {
        self.stores.get_mut(name)
    }

    pub fn store_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.stores.keys().map(|s| s.as_str()).collect();
        names.sort_unstable();
        names
    }

    pub fn len(&self) -> usize {
        self.stores.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stores.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────────────
//  Wire format — durable batch snapshots (§108.x, D108.8)
// ─────────────────────────────────────────────────────────────────────
//
// A batch is IMMUTABLE, so persistence is trivially incremental: a
// batch, once written, never changes. The wire form is deliberately
// dumb JSON (deterministic, diffable, no format-versioning games in
// v1); the DESERIALIZATION path re-runs every §5.1 invariant through
// [`ColumnArray::validated`] — the single choke point — so a tampered
// or truncated snapshot is REFUSED, never half-loaded.

/// One column's buffers on the wire. Bytes are base64 (JSON-safe).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WireColumn {
    pub column_type: String,
    pub validity_b64: String,
    /// Fixed-width payloads (Int/Float/Timestamp as little-endian 8-byte
    /// lanes; Bool bit-packed) OR the raw byte buffer (Text/Json).
    pub data_b64: String,
    /// Present only for variable-width columns (Text/Json).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offsets: Option<Vec<u64>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WireBatch {
    pub n: usize,
    pub columns: Vec<WireColumn>,
    pub source: String,
    pub source_sha256: String,
    pub ingested_at: String,
    /// `untrusted` | `schema_validated` | `elevated`.
    pub taint: String,
}

fn b64_encode(bytes: &[u8]) -> String {
    // First-party base64 (standard alphabet, padded) — 20 lines beat a
    // dependency for a cold path.
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let v = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(A[(v >> 18) as usize & 63] as char);
        out.push(A[(v >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { A[(v >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { A[v as usize & 63] as char } else { '=' });
    }
    out
}

fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Result<u32, String> {
        match c {
            b'A'..=b'Z' => Ok((c - b'A') as u32),
            b'a'..=b'z' => Ok((c - b'a') as u32 + 26),
            b'0'..=b'9' => Ok((c - b'0') as u32 + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(format!("invalid base64 byte 0x{c:02x}")),
        }
    }
    let s = s.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for chunk in s.chunks(4) {
        let mut v: u32 = 0;
        for (i, &c) in chunk.iter().enumerate() {
            v |= val(c)? << (18 - 6 * i);
        }
        out.push((v >> 16) as u8);
        if chunk.len() > 2 {
            out.push((v >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(v as u8);
        }
    }
    Ok(out)
}

impl RecordBatch {
    /// Serialize for durable snapshotting (§108.x). Loss-free: the wire
    /// form carries the exact buffers + the provenance stamp.
    pub fn to_wire(&self) -> WireBatch {
        let columns = self
            .columns
            .iter()
            .map(|c| {
                let (data, offsets) = match &c.data {
                    ColumnData::Int(v) | ColumnData::Timestamp(v) => (
                        v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>(),
                        None,
                    ),
                    ColumnData::Float(v) => (
                        v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>(),
                        None,
                    ),
                    ColumnData::Bool(v) => (v.clone(), None),
                    ColumnData::Text { offsets, bytes } | ColumnData::Json { offsets, bytes } => {
                        (bytes.clone(), Some(offsets.clone()))
                    }
                };
                WireColumn {
                    column_type: c.column_type().canonical_name().to_string(),
                    validity_b64: b64_encode(&c.validity),
                    data_b64: b64_encode(&data),
                    offsets,
                }
            })
            .collect();
        WireBatch {
            n: self.n,
            columns,
            source: self.provenance.source.clone(),
            source_sha256: self.provenance.source_sha256.clone(),
            ingested_at: self.provenance.ingested_at.clone(),
            taint: taint_str(self.provenance.taint).to_string(),
        }
    }

    /// Rebuild from the wire AGAINST the declared schema, re-running
    /// every §5.1 invariant ([`ColumnArray::validated`]) + the batch
    /// schema/length checks. A tampered, truncated or schema-drifted
    /// snapshot is REFUSED whole — never half-loaded.
    pub fn from_wire(
        schema: &[(String, ColumnType)],
        wire: &WireBatch,
    ) -> Result<RecordBatch, String> {
        let taint = match wire.taint.as_str() {
            "untrusted" => EpistemicTaint::Untrusted,
            "schema_validated" => EpistemicTaint::SchemaValidated,
            "elevated" => EpistemicTaint::Elevated,
            other => return Err(format!("unknown taint `{other}` in snapshot")),
        };
        let mut columns = Vec::with_capacity(wire.columns.len());
        for wc in &wire.columns {
            let ty = ColumnType::from_canonical(&wc.column_type)
                .ok_or_else(|| format!("unknown column type `{}` in snapshot", wc.column_type))?;
            let validity = b64_decode(&wc.validity_b64)?;
            let raw = b64_decode(&wc.data_b64)?;
            let data = match ty {
                ColumnType::Int | ColumnType::Timestamp => {
                    if raw.len() % 8 != 0 {
                        return Err("snapshot Int/Timestamp buffer not 8-byte aligned".into());
                    }
                    let v: Vec<i64> = raw
                        .chunks_exact(8)
                        .map(|c| i64::from_le_bytes(c.try_into().unwrap()))
                        .collect();
                    if ty == ColumnType::Int {
                        ColumnData::Int(v)
                    } else {
                        ColumnData::Timestamp(v)
                    }
                }
                ColumnType::Float => {
                    if raw.len() % 8 != 0 {
                        return Err("snapshot Float buffer not 8-byte aligned".into());
                    }
                    ColumnData::Float(
                        raw.chunks_exact(8)
                            .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
                            .collect(),
                    )
                }
                ColumnType::Bool => ColumnData::Bool(raw),
                ColumnType::Text => ColumnData::Text {
                    offsets: wc.offsets.clone().ok_or("Text column missing offsets")?,
                    bytes: raw,
                },
                ColumnType::Json => ColumnData::Json {
                    offsets: wc.offsets.clone().ok_or("Json column missing offsets")?,
                    bytes: raw,
                },
            };
            columns.push(ColumnArray::validated(wire.n, validity, data)?);
        }
        RecordBatch::new(
            schema,
            columns,
            BatchProvenance {
                source: wire.source.clone(),
                source_sha256: wire.source_sha256.clone(),
                ingested_at: wire.ingested_at.clone(),
                taint,
            },
        )
    }
}

// ─────────────────────────────────────────────────────────────────────
//  Governed ingest — deterministic loaders (§108.c)
// ─────────────────────────────────────────────────────────────────────

/// The closed loader catalog (axon-T929). Deterministic + first-party
/// — the §100 posture. Parquet / Arrow-IPC are deferred §108.x surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestFormat {
    Csv,
    Json,
}

impl IngestFormat {
    pub fn from_declared(s: &str) -> Option<IngestFormat> {
        match s {
            "csv" => Some(IngestFormat::Csv),
            "json" => Some(IngestFormat::Json),
            _ => None,
        }
    }
}

/// Bounds enforced on the RAW byte stream BEFORE any parsing (§100 —
/// bounds-BEFORE-parse). The defaults are deliberately conservative:
/// an ingest is bounded BY DEFAULT, never unbounded.
#[derive(Debug, Clone, Copy)]
pub struct IngestLimits {
    pub max_bytes: u64,
    pub max_rows: u64,
}

impl Default for IngestLimits {
    fn default() -> Self {
        IngestLimits {
            max_bytes: 16 * 1024 * 1024, // 16 MiB
            max_rows: 1_000_000,
        }
    }
}

/// Parse + type raw source bytes into ONE immutable [`RecordBatch`]
/// against the declared schema.
///
/// The §108.c laws, in order:
/// 1. **Bounds BEFORE parse** (§100): the byte bound is checked against
///    the raw stream before a single byte is interpreted; the row bound
///    during parsing, before typing each excess row.
/// 2. **Type refusal, not coercion** (D108.7): a value that does not
///    fit its column type refuses the WHOLE batch, naming row + column.
///    A missing value (empty CSV field / absent JSON key / JSON null)
///    is a structural null in the validity bitmap — the only
///    flexibility.
/// 3. The batch is stamped with `provenance` (born-Untrusted, §98) —
///    stamping happens HERE so no unstamped batch can exist.
pub fn ingest_bytes(
    schema: &[(String, ColumnType)],
    format: IngestFormat,
    raw: &[u8],
    limits: &IngestLimits,
    provenance: BatchProvenance,
) -> Result<RecordBatch, String> {
    // Law 1 — the byte bound, before ANY interpretation of the stream.
    if raw.len() as u64 > limits.max_bytes {
        return Err(format!(
            "ingest refused BEFORE parse: source is {} bytes, the declared bound is {} \
             (bounds-BEFORE-parse — raise `limits {{ max_bytes: … }}` if intended)",
            raw.len(),
            limits.max_bytes
        ));
    }
    let mut builders: Vec<ColumnBuilder> = schema
        .iter()
        .map(|(_, ty)| ColumnBuilder::new(*ty))
        .collect();
    match format {
        IngestFormat::Csv => fill_from_csv(schema, raw, limits, &mut builders)?,
        IngestFormat::Json => fill_from_json(schema, raw, limits, &mut builders)?,
    }
    let columns: Vec<ColumnArray> = builders.into_iter().map(|b| b.finish()).collect();
    RecordBatch::new(schema, columns, provenance)
}

/// Minimal deterministic CSV reader (RFC 4180 core): quoted fields,
/// `""` escapes inside quotes, commas + LF/CRLF inside quotes, header
/// row REQUIRED. Returns rows of raw string fields.
fn parse_csv_rows(text: &str) -> Result<Vec<Vec<String>>, String> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' => {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        field.push('"');
                    } else {
                        in_quotes = false;
                    }
                }
                other => field.push(other),
            }
        } else {
            match c {
                '"' => {
                    if field.is_empty() {
                        in_quotes = true;
                    } else {
                        return Err(format!(
                            "malformed CSV at row {}: a quote may only open an empty field",
                            rows.len() + 1
                        ));
                    }
                }
                ',' => {
                    row.push(std::mem::take(&mut field));
                }
                '\r' => { /* swallowed; the LF closes the record */ }
                '\n' => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                }
                other => field.push(other),
            }
        }
    }
    if in_quotes {
        return Err("malformed CSV: unclosed quoted field at end of input".to_string());
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

fn fill_from_csv(
    schema: &[(String, ColumnType)],
    raw: &[u8],
    limits: &IngestLimits,
    builders: &mut [ColumnBuilder],
) -> Result<(), String> {
    let text = std::str::from_utf8(raw).map_err(|_| "source is not valid UTF-8".to_string())?;
    let rows = parse_csv_rows(text)?;
    let Some((header, data_rows)) = rows.split_first() else {
        return Ok(()); // empty source → empty batch (0 rows is honest)
    };
    // Column mapping is BY NAME: every schema column must appear in the
    // header (a load that silently fills a declared column with nulls
    // because the header lacks it would be laundering); EXTRA source
    // columns are projected away (π, not coercion).
    let mut col_idx: Vec<usize> = Vec::with_capacity(schema.len());
    for (name, _) in schema {
        match header.iter().position(|h| h.trim() == name) {
            Some(i) => col_idx.push(i),
            None => {
                return Err(format!(
                    "CSV header does not contain declared column `{name}` — the header is \
                     {header:?}. Every schema column must be present (extra source columns \
                     are ignored; missing ones are refused, not null-filled)."
                ));
            }
        }
    }
    // Law 1 (rows) — bound checked BEFORE typing any excess row.
    if data_rows.len() as u64 > limits.max_rows {
        return Err(format!(
            "ingest refused: source carries {} rows, the declared bound is {} \
             (`limits {{ max_rows: … }}`)",
            data_rows.len(),
            limits.max_rows
        ));
    }
    for (r, row) in data_rows.iter().enumerate() {
        for (s, (name, ty)) in schema.iter().enumerate() {
            let raw_field = row.get(col_idx[s]).map(|f| f.as_str()).unwrap_or("");
            push_csv_field(&mut builders[s], *ty, raw_field)
                .map_err(|e| format!("row {} column `{name}`: {e}", r + 1))?;
        }
    }
    Ok(())
}

fn push_csv_field(b: &mut ColumnBuilder, ty: ColumnType, field: &str) -> Result<(), String> {
    if field.is_empty() {
        b.push_null(); // empty field = structural null (the ONLY flexibility)
        return Ok(());
    }
    match ty {
        ColumnType::Int => {
            let v: i64 = field
                .trim()
                .parse()
                .map_err(|_| format!("expected Int, got `{field}` (refusal, not coercion)"))?;
            b.push_int(v)
        }
        ColumnType::Float => {
            let v: f64 = field
                .trim()
                .parse()
                .map_err(|_| format!("expected Float, got `{field}` (refusal, not coercion)"))?;
            b.push_float(v)
        }
        ColumnType::Bool => match field.trim().to_ascii_lowercase().as_str() {
            "true" => b.push_bool(true),
            "false" => b.push_bool(false),
            other => Err(format!(
                "expected Bool (`true`/`false`), got `{other}` (refusal, not coercion)"
            )),
        },
        ColumnType::Timestamp => {
            let dt = chrono::DateTime::parse_from_rfc3339(field.trim()).map_err(|_| {
                format!("expected an RFC 3339 timestamp, got `{field}` (refusal, not coercion)")
            })?;
            b.push_int(dt.timestamp_micros())
        }
        ColumnType::Text => b.push_text(field),
        ColumnType::Json => {
            let v: serde_json::Value = serde_json::from_str(field)
                .map_err(|_| format!("expected valid JSON, got `{field}`"))?;
            // Re-serialized compact — ONE canonical byte form per value.
            b.push_json_bytes(v.to_string().as_bytes())
        }
    }
}

fn fill_from_json(
    schema: &[(String, ColumnType)],
    raw: &[u8],
    limits: &IngestLimits,
    builders: &mut [ColumnBuilder],
) -> Result<(), String> {
    let root: serde_json::Value =
        serde_json::from_slice(raw).map_err(|e| format!("source is not valid JSON: {e}"))?;
    let rows = root
        .as_array()
        .ok_or_else(|| "JSON source must be an ARRAY of row objects".to_string())?;
    if rows.len() as u64 > limits.max_rows {
        return Err(format!(
            "ingest refused: source carries {} rows, the declared bound is {} \
             (`limits {{ max_rows: … }}`)",
            rows.len(),
            limits.max_rows
        ));
    }
    for (r, row) in rows.iter().enumerate() {
        let obj = row.as_object().ok_or_else(|| {
            format!("row {}: every JSON row must be an object", r + 1)
        })?;
        for (s, (name, ty)) in schema.iter().enumerate() {
            let value = obj.get(name.as_str()).unwrap_or(&serde_json::Value::Null);
            push_json_field(&mut builders[s], *ty, value)
                .map_err(|e| format!("row {} column `{name}`: {e}", r + 1))?;
        }
    }
    Ok(())
}

fn push_json_field(
    b: &mut ColumnBuilder,
    ty: ColumnType,
    v: &serde_json::Value,
) -> Result<(), String> {
    if v.is_null() {
        b.push_null(); // absent key or explicit null = structural null
        return Ok(());
    }
    match ty {
        ColumnType::Int => match v.as_i64() {
            Some(i) => b.push_int(i),
            None => Err(format!(
                "expected Int, got {v} (a fractional number or non-number is a refusal, \
                 not a coercion)"
            )),
        },
        // JSON has ONE number type — an integer literal in a Float
        // column is the same JSON number, not a coercion.
        ColumnType::Float => match v.as_f64() {
            Some(f) => b.push_float(f),
            None => Err(format!("expected Float, got {v} (refusal, not coercion)")),
        },
        ColumnType::Bool => match v.as_bool() {
            Some(x) => b.push_bool(x),
            None => Err(format!("expected Bool, got {v} (refusal, not coercion)")),
        },
        ColumnType::Timestamp => match v.as_str() {
            Some(s) => {
                let dt = chrono::DateTime::parse_from_rfc3339(s).map_err(|_| {
                    format!("expected an RFC 3339 timestamp string, got `{s}`")
                })?;
                b.push_int(dt.timestamp_micros())
            }
            None => Err(format!("expected an RFC 3339 timestamp string, got {v}")),
        },
        ColumnType::Text => match v.as_str() {
            Some(s) => b.push_text(s),
            None => Err(format!(
                "expected Text, got {v} (a number is not silently stringified — \
                 refusal, not coercion)"
            )),
        },
        ColumnType::Json => b.push_json_bytes(v.to_string().as_bytes()),
    }
}

// ─────────────────────────────────────────────────────────────────────
//  The lazy relational query engine (§108.d)
// ─────────────────────────────────────────────────────────────────────
//
// σ (focus) · γ (aggregate) · equi-⋈ (associate) · profile (explore),
// with predicates drawn from the ONE data-plane `where:` grammar the
// product already ships — `crate::store::filter` (§35.b: closed,
// whitelisted operators, `${name}` bindings resolved inside tokenized
// string literals, fuzzed §35.k). D108.9: `retrieve`, `navigate` and
// the dataspace verbs share a single `where:` surface; the §70 `Expr`
// engine stays the CONTROL-PLANE expression language (`if` / `let`).
//
// Evaluation is in-memory over the columnar batches, with SQL
// precedence (AND binds tighter than OR) and SQL null semantics
// (`= NULL` ⇒ is-null; ordering/LIKE against NULL ⇒ no match).
// Batches are pruned through the zone-map abstraction (§5.3): a batch
// is skipped ONLY when the predicate is PROVABLY false over its
// per-column `[min, max]` — sound by construction, completeness not
// claimed (a `maybe` batch is scanned).

use crate::store::filter::{
    Connector as FConnector, Filter, FilterCondition, Operator as FOp, Rhs, SqlValue,
};

/// One materialized cell surfaced by a scan.
#[derive(Debug, Clone, PartialEq)]
pub enum CellValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Text(String),
    /// Epoch microseconds (rendered as RFC 3339 UTC at the JSON edge).
    Timestamp(i64),
    Json(serde_json::Value),
    Null,
}

fn cell_at(col: &ColumnArray, i: usize) -> CellValue {
    if !col.is_valid(i) {
        return CellValue::Null;
    }
    match col.column_type() {
        ColumnType::Int => CellValue::Int(col.get_int(i).unwrap()),
        ColumnType::Float => CellValue::Float(col.get_float(i).unwrap()),
        ColumnType::Bool => CellValue::Bool(col.get_bool(i).unwrap()),
        ColumnType::Timestamp => CellValue::Timestamp(col.get_int(i).unwrap()),
        ColumnType::Text => CellValue::Text(col.get_text(i).unwrap_or_default().to_string()),
        ColumnType::Json => CellValue::Json(
            col.get_bytes(i)
                .and_then(|b| serde_json::from_slice(b).ok())
                .unwrap_or(serde_json::Value::Null),
        ),
    }
}

fn micros_to_rfc3339(us: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_micros(us)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| us.to_string())
}

fn cell_to_json(c: &CellValue) -> serde_json::Value {
    match c {
        CellValue::Int(v) => serde_json::json!(v),
        CellValue::Float(v) => serde_json::json!(v),
        CellValue::Bool(v) => serde_json::json!(v),
        CellValue::Text(v) => serde_json::json!(v),
        CellValue::Timestamp(us) => serde_json::json!(micros_to_rfc3339(*us)),
        CellValue::Json(v) => v.clone(),
        CellValue::Null => serde_json::Value::Null,
    }
}

/// Minimal deterministic SQL `LIKE` (`%` any-run, `_` any-one), the
/// §35.b surface. Case-sensitive (documented; ILIKE is deferred).
fn like_match(text: &str, pattern: &str) -> bool {
    fn rec(t: &[char], p: &[char]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some('%') => (0..=t.len()).any(|k| rec(&t[k..], &p[1..])),
            Some('_') => !t.is_empty() && rec(&t[1..], &p[1..]),
            Some(c) => t.first() == Some(c) && rec(&t[1..], &p[1..]),
        }
    }
    let t: Vec<char> = text.chars().collect();
    let p: Vec<char> = pattern.chars().collect();
    rec(&t, &p)
}

/// Evaluate one `column op value` against a cell. `Err` = a DECLARATION
/// error (type mismatch between the condition and the column) — fail
/// closed, never a silent `false`.
fn eval_condition(cond: &FilterCondition, cell: &CellValue) -> Result<bool, String> {
    let rhs = match &cond.value {
        Rhs::Value(v) => v,
        Rhs::Time(_) => {
            return Err(format!(
                "column `{}`: `now()` time values in a dataspace `where:` are deferred \
                 surface — bind an RFC 3339 literal instead",
                cond.column
            ))
        }
    };
    // SQL null semantics: `= NULL` is-null / `!= NULL` is-not-null;
    // any other combination involving NULL matches nothing.
    if matches!(rhs, SqlValue::Null) {
        return match cond.op {
            FOp::Eq => Ok(matches!(cell, CellValue::Null)),
            FOp::Ne => Ok(!matches!(cell, CellValue::Null)),
            _ => Ok(false),
        };
    }
    if matches!(cell, CellValue::Null) {
        return Ok(matches!(cond.op, FOp::Ne)); // NULL != <literal> is true; the rest match nothing
    }
    let ord: std::cmp::Ordering = match (cell, rhs) {
        (CellValue::Int(a), SqlValue::Integer(b)) => a.cmp(b),
        (CellValue::Int(a), SqlValue::Float(b)) => {
            (*a as f64).partial_cmp(b).ok_or("NaN comparison")?
        }
        (CellValue::Float(a), SqlValue::Integer(b)) => {
            a.partial_cmp(&(*b as f64)).ok_or("NaN comparison")?
        }
        (CellValue::Float(a), SqlValue::Float(b)) => a.partial_cmp(b).ok_or("NaN comparison")?,
        (CellValue::Text(a), SqlValue::Text(b)) => {
            if cond.op == FOp::Like {
                return Ok(like_match(a, b));
            }
            a.as_str().cmp(b.as_str())
        }
        (CellValue::Bool(a), SqlValue::Boolean(b)) => {
            return match cond.op {
                FOp::Eq => Ok(a == b),
                FOp::Ne => Ok(a != b),
                other => Err(format!(
                    "column `{}`: ordering operator {other} is not defined on Bool",
                    cond.column
                )),
            };
        }
        (CellValue::Timestamp(a), SqlValue::Text(b)) => {
            let rhs_us = chrono::DateTime::parse_from_rfc3339(b)
                .map_err(|_| {
                    format!(
                        "column `{}`: a Timestamp compares against an RFC 3339 literal, \
                         got `{b}`",
                        cond.column
                    )
                })?
                .timestamp_micros();
            a.cmp(&rhs_us)
        }
        (CellValue::Timestamp(a), SqlValue::Integer(b)) => a.cmp(b),
        (cell, rhs) => {
            return Err(format!(
                "column `{}`: type mismatch — a {} column against a {} literal \
                 (refusal, not coercion)",
                cond.column,
                match cell {
                    CellValue::Int(_) => "Int",
                    CellValue::Float(_) => "Float",
                    CellValue::Bool(_) => "Bool",
                    CellValue::Text(_) => "Text",
                    CellValue::Timestamp(_) => "Timestamp",
                    CellValue::Json(_) => "Json",
                    CellValue::Null => "Null",
                },
                rhs.type_name()
            ))
        }
    };
    Ok(match cond.op {
        FOp::Eq => ord == std::cmp::Ordering::Equal,
        FOp::Ne => ord != std::cmp::Ordering::Equal,
        FOp::Gt => ord == std::cmp::Ordering::Greater,
        FOp::Ge => ord != std::cmp::Ordering::Less,
        FOp::Lt => ord == std::cmp::Ordering::Less,
        FOp::Le => ord != std::cmp::Ordering::Greater,
        FOp::Like => {
            return Err(format!(
                "column `{}`: LIKE is defined on Text only",
                cond.column
            ))
        }
    })
}

/// SQL precedence over the flat condition list: AND binds tighter than
/// OR — the filter is an OR of AND-groups.
fn eval_filter_on_row(
    filter: &Filter,
    row_cell: impl Fn(&str) -> Result<CellValue, String>,
) -> Result<bool, String> {
    if filter.conditions.is_empty() {
        return Ok(true);
    }
    let mut any_group = false;
    let mut group = true;
    for (i, cond) in filter.conditions.iter().enumerate() {
        if group {
            let cell = row_cell(&cond.column)?;
            group = eval_condition(cond, &cell)?;
        }
        match filter.connectors.get(i) {
            Some(FConnector::And) => {}
            Some(FConnector::Or) => {
                any_group = any_group || group;
                group = true;
            }
            None => {}
        }
    }
    Ok(any_group || group)
}

/// §5.3 — the interval abstraction φ̂ for ONE condition over a batch's
/// zone map. `true` = maybe (scan), `false` = PROVABLY no row in the
/// batch satisfies it. Conservative by construction: anything without
/// a precise abstraction answers `maybe`.
fn condition_maybe(cond: &FilterCondition, zm: &ZoneMap, batch_len: usize) -> bool {
    let rhs = match &cond.value {
        Rhs::Value(v) => v,
        Rhs::Time(_) => return true,
    };
    if matches!(rhs, SqlValue::Null) {
        return match cond.op {
            FOp::Eq => zm.null_count > 0,
            FOp::Ne => zm.null_count < batch_len,
            _ => true,
        };
    }
    // A batch with nulls can always satisfy `!=` via NULL semantics.
    let nulls_present = zm.null_count > 0;
    let maybe = match (&zm.stats, rhs) {
        (ZoneStats::Int { min, max }, SqlValue::Integer(v)) => range_maybe(cond.op, *min, *max, *v),
        (ZoneStats::Int { min, max }, SqlValue::Float(v)) => {
            range_maybe_f(cond.op, *min as f64, *max as f64, *v)
        }
        (ZoneStats::Float { min, max }, SqlValue::Float(v)) => range_maybe_f(cond.op, *min, *max, *v),
        (ZoneStats::Float { min, max }, SqlValue::Integer(v)) => {
            range_maybe_f(cond.op, *min, *max, *v as f64)
        }
        (ZoneStats::Text { min, max }, SqlValue::Text(v)) => {
            if cond.op == FOp::Like {
                true
            } else {
                range_maybe_ord(cond.op, min.as_str(), max.as_str(), v.as_str())
            }
        }
        (ZoneStats::Bool { any_true, any_false }, SqlValue::Boolean(v)) => match cond.op {
            FOp::Eq => {
                if *v {
                    *any_true
                } else {
                    *any_false
                }
            }
            FOp::Ne => {
                if *v {
                    *any_false
                } else {
                    *any_true
                }
            }
            _ => true,
        },
        // Timestamp zones live in ZoneStats::Int (epoch micros).
        (ZoneStats::Int { min, max }, SqlValue::Text(v)) => {
            match chrono::DateTime::parse_from_rfc3339(v) {
                Ok(dt) => range_maybe(cond.op, *min, *max, dt.timestamp_micros()),
                Err(_) => true, // the row eval will refuse loudly
            }
        }
        _ => true,
    };
    maybe || (nulls_present && matches!(cond.op, FOp::Ne))
}

fn range_maybe(op: FOp, min: i64, max: i64, v: i64) -> bool {
    match op {
        FOp::Eq => min <= v && v <= max,
        FOp::Ne => !(min == v && max == v),
        FOp::Gt => max > v,
        FOp::Ge => max >= v,
        FOp::Lt => min < v,
        FOp::Le => min <= v,
        FOp::Like => true,
    }
}

fn range_maybe_f(op: FOp, min: f64, max: f64, v: f64) -> bool {
    match op {
        FOp::Eq => min <= v && v <= max,
        FOp::Ne => !(min == v && max == v),
        FOp::Gt => max > v,
        FOp::Ge => max >= v,
        FOp::Lt => min < v,
        FOp::Le => min <= v,
        FOp::Like => true,
    }
}

fn range_maybe_ord(op: FOp, min: &str, max: &str, v: &str) -> bool {
    match op {
        FOp::Eq => min <= v && v <= max,
        FOp::Ne => !(min == v && max == v),
        FOp::Gt => max > v,
        FOp::Ge => max >= v,
        FOp::Lt => min < v,
        FOp::Le => min <= v,
        FOp::Like => true,
    }
}

/// φ̂ over a whole batch: with AND > OR, the batch is prunable iff EVERY
/// OR-group contains at least one PROVABLY-false condition.
fn batch_maybe(filter: &Filter, batch: &RecordBatch, schema: &[(String, ColumnType)]) -> bool {
    if filter.conditions.is_empty() {
        return true;
    }
    let mut group_maybe = true;
    for (i, cond) in filter.conditions.iter().enumerate() {
        let cond_ok = match schema.iter().position(|(n, _)| n == &cond.column) {
            Some(idx) => match batch.zone_map(idx) {
                Some(zm) => condition_maybe(cond, zm, batch.len()),
                None => true,
            },
            None => true, // unknown column → the row eval refuses loudly
        };
        group_maybe = group_maybe && cond_ok;
        match filter.connectors.get(i) {
            Some(FConnector::Or) => {
                if group_maybe {
                    return true; // one live OR-group ⇒ scan
                }
                group_maybe = true;
            }
            _ => {}
        }
    }
    group_maybe
}

/// Deterministic scan statistics — surfaced in every query summary so
/// pruning is OBSERVABLE, never a silent claim.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct QueryStats {
    pub batches_total: usize,
    pub batches_pruned: usize,
    pub rows_scanned: usize,
    pub rows_matched: usize,
}

/// A query result: JSON rows + the epistemic meet + scan stats.
#[derive(Debug, Clone)]
pub struct QueryOutput {
    pub rows: serde_json::Value,
    pub taint: EpistemicTaint,
    pub stats: QueryStats,
}

/// The §5.4 meet over a store — conservative: the result's status is
/// the meet over ALL of the store's batches (a strictly-lower-or-equal
/// bound vs. touched-only; the conservative direction is always sound).
/// An empty store yields the floor (`Untrusted`).
fn store_taint(store: &DataspaceStore) -> EpistemicTaint {
    store
        .batches()
        .iter()
        .map(|b| b.provenance().taint)
        .fold(None, |acc: Option<EpistemicTaint>, t| {
            Some(match acc {
                None => t,
                Some(a) => taint_meet(a, t),
            })
        })
        .unwrap_or(EpistemicTaint::Untrusted)
}

fn parse_where(
    store: &DataspaceStore,
    where_str: &str,
    bindings: &std::collections::HashMap<String, String>,
) -> Result<Filter, String> {
    if where_str.trim().is_empty() {
        return Ok(Filter {
            conditions: Vec::new(),
            connectors: Vec::new(),
        });
    }
    let filter = crate::store::filter::parse_filter(where_str, bindings)
        .map_err(|e| format!("where clause: {e}"))?;
    // Every referenced column must be declared — fail closed BEFORE the scan.
    for cond in &filter.conditions {
        if store.column_index(&cond.column).is_none() {
            return Err(format!(
                "where clause references `{}`, which is not a column of dataspace `{}` \
                 (declared: {})",
                cond.column,
                store.name,
                store
                    .schema()
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    Ok(filter)
}

/// σ_φ ∘ π_v — `focus` (§108.d). Scans batches the zone maps cannot
/// refute, evaluates φ per row, projects `select` (empty ⇒ all columns,
/// in declaration order).
pub fn focus_query(
    store: &DataspaceStore,
    where_str: &str,
    select: &[String],
    bindings: &std::collections::HashMap<String, String>,
) -> Result<QueryOutput, String> {
    let filter = parse_where(store, where_str, bindings)?;
    let schema = store.schema();
    let proj: Vec<usize> = if select.is_empty() {
        (0..schema.len()).collect()
    } else {
        select
            .iter()
            .map(|name| {
                store.column_index(name).ok_or_else(|| {
                    format!(
                        "select references `{name}`, which is not a column of dataspace `{}`",
                        store.name
                    )
                })
            })
            .collect::<Result<_, _>>()?
    };
    let mut stats = QueryStats {
        batches_total: store.batches().len(),
        ..Default::default()
    };
    let mut out: Vec<serde_json::Value> = Vec::new();
    for batch in store.batches() {
        if !batch_maybe(&filter, batch, schema) {
            stats.batches_pruned += 1;
            continue;
        }
        for i in 0..batch.len() {
            stats.rows_scanned += 1;
            let hit = eval_filter_on_row(&filter, |col| {
                let idx = store
                    .column_index(col)
                    .ok_or_else(|| format!("unknown column `{col}`"))?;
                Ok(cell_at(batch.column(idx).unwrap(), i))
            })?;
            if hit {
                stats.rows_matched += 1;
                let mut obj = serde_json::Map::new();
                for &c in &proj {
                    obj.insert(
                        schema[c].0.clone(),
                        cell_to_json(&cell_at(batch.column(c).unwrap(), i)),
                    );
                }
                out.push(serde_json::Value::Object(obj));
            }
        }
    }
    Ok(QueryOutput {
        rows: serde_json::Value::Array(out),
        taint: store_taint(store),
        stats,
    })
}

/// The closed aggregate catalog (§108.d).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggFunc {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

#[derive(Debug, Clone)]
pub struct AggregateSpec {
    pub func: AggFunc,
    /// `None` only for `count` (row count).
    pub column: Option<String>,
}

impl AggregateSpec {
    /// Parse the declared form: `count` | `count(col)` | `sum(col)` |
    /// `avg(col)` | `min(col)` | `max(col)`.
    pub fn parse(s: &str) -> Result<AggregateSpec, String> {
        let s = s.trim();
        let (name, col) = match s.find('(') {
            Some(p) if s.ends_with(')') => (
                &s[..p],
                Some(s[p + 1..s.len() - 1].trim().to_string()).filter(|c| !c.is_empty()),
            ),
            None => (s, None),
            _ => return Err(format!("malformed aggregate `{s}`")),
        };
        let func = match name.trim() {
            "count" => AggFunc::Count,
            "sum" => AggFunc::Sum,
            "avg" => AggFunc::Avg,
            "min" => AggFunc::Min,
            "max" => AggFunc::Max,
            other => {
                return Err(format!(
                    "unknown aggregate `{other}` — the closed catalog is \
                     {{count, sum, avg, min, max}}"
                ))
            }
        };
        if func != AggFunc::Count && col.is_none() {
            return Err(format!("aggregate `{name}` requires a column: `{name}(<col>)`"));
        }
        Ok(AggregateSpec { func, column: col })
    }

    fn output_key(&self) -> String {
        match (&self.func, &self.column) {
            (AggFunc::Count, None) => "count".to_string(),
            (f, Some(c)) => format!(
                "{}_{c}",
                match f {
                    AggFunc::Count => "count",
                    AggFunc::Sum => "sum",
                    AggFunc::Avg => "avg",
                    AggFunc::Min => "min",
                    AggFunc::Max => "max",
                }
            ),
            (_, None) => unreachable!("a non-count aggregate without a column is refused at parse"),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct AggState {
    count: u64,
    sum: f64,
    min_f: Option<f64>,
    max_f: Option<f64>,
    min_s: Option<String>,
    max_s: Option<String>,
}

/// γ — `aggregate` (§108.d). Groups by `group_by` (empty ⇒ one global
/// group) and computes the closed catalog. Nulls are skipped by every
/// column-aggregate (`count` with no column counts ROWS). Output rows
/// are sorted by group key — DETERMINISTIC output, always.
pub fn aggregate_query(
    store: &DataspaceStore,
    where_str: &str,
    group_by: &[String],
    computes: &[AggregateSpec],
    bindings: &std::collections::HashMap<String, String>,
) -> Result<QueryOutput, String> {
    let filter = parse_where(store, where_str, bindings)?;
    let schema = store.schema();
    // Validate the referenced columns + numeric discipline up front.
    let group_idx: Vec<usize> = group_by
        .iter()
        .map(|g| {
            store.column_index(g).ok_or_else(|| {
                format!("group_by references `{g}`, not a column of `{}`", store.name)
            })
        })
        .collect::<Result<_, _>>()?;
    for spec in computes {
        if let Some(col) = &spec.column {
            let idx = store.column_index(col).ok_or_else(|| {
                format!("aggregate references `{col}`, not a column of `{}`", store.name)
            })?;
            let ty = schema[idx].1;
            match spec.func {
                AggFunc::Sum | AggFunc::Avg => {
                    if !matches!(ty, ColumnType::Int | ColumnType::Float) {
                        return Err(format!(
                            "`{}` is defined on Int/Float; column `{col}` is {} \
                             (refusal, not coercion)",
                            spec.output_key(),
                            ty.canonical_name()
                        ));
                    }
                }
                AggFunc::Min | AggFunc::Max => {
                    if matches!(ty, ColumnType::Json | ColumnType::Bool) {
                        return Err(format!(
                            "`{}` is not defined on {} columns",
                            spec.output_key(),
                            ty.canonical_name()
                        ));
                    }
                }
                AggFunc::Count => {}
            }
        }
    }
    let mut stats = QueryStats {
        batches_total: store.batches().len(),
        ..Default::default()
    };
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<String, (Vec<serde_json::Value>, Vec<AggState>)> = BTreeMap::new();
    for batch in store.batches() {
        if !batch_maybe(&filter, batch, schema) {
            stats.batches_pruned += 1;
            continue;
        }
        for i in 0..batch.len() {
            stats.rows_scanned += 1;
            let hit = eval_filter_on_row(&filter, |col| {
                let idx = store
                    .column_index(col)
                    .ok_or_else(|| format!("unknown column `{col}`"))?;
                Ok(cell_at(batch.column(idx).unwrap(), i))
            })?;
            if !hit {
                continue;
            }
            stats.rows_matched += 1;
            let key_cells: Vec<serde_json::Value> = group_idx
                .iter()
                .map(|&g| cell_to_json(&cell_at(batch.column(g).unwrap(), i)))
                .collect();
            let key = serde_json::Value::Array(key_cells.clone()).to_string();
            let entry = groups
                .entry(key)
                .or_insert_with(|| (key_cells, vec![AggState::default(); computes.len()]));
            for (s, spec) in computes.iter().enumerate() {
                let st = &mut entry.1[s];
                match &spec.column {
                    None => st.count += 1, // count(*) — rows
                    Some(col) => {
                        let idx = store.column_index(col).unwrap();
                        match cell_at(batch.column(idx).unwrap(), i) {
                            CellValue::Null => {} // nulls are skipped
                            CellValue::Int(v) => {
                                st.count += 1;
                                st.sum += v as f64;
                                let f = v as f64;
                                st.min_f = Some(st.min_f.map_or(f, |m| m.min(f)));
                                st.max_f = Some(st.max_f.map_or(f, |m| m.max(f)));
                            }
                            CellValue::Float(v) => {
                                st.count += 1;
                                st.sum += v;
                                st.min_f = Some(st.min_f.map_or(v, |m| m.min(v)));
                                st.max_f = Some(st.max_f.map_or(v, |m| m.max(v)));
                            }
                            CellValue::Timestamp(v) => {
                                st.count += 1;
                                let f = v as f64;
                                st.min_f = Some(st.min_f.map_or(f, |m| m.min(f)));
                                st.max_f = Some(st.max_f.map_or(f, |m| m.max(f)));
                            }
                            CellValue::Text(t) => {
                                st.count += 1;
                                st.min_s = Some(match st.min_s.take() {
                                    None => t.clone(),
                                    Some(m) => m.min(t.clone()),
                                });
                                st.max_s = Some(match st.max_s.take() {
                                    None => t.clone(),
                                    Some(m) => m.max(t),
                                });
                            }
                            CellValue::Bool(_) | CellValue::Json(_) => {
                                st.count += 1; // count admits any type; sum/min/max were gated
                            }
                        }
                    }
                }
            }
        }
    }
    let mut out: Vec<serde_json::Value> = Vec::new();
    for (_key, (key_cells, states)) in groups {
        let mut obj = serde_json::Map::new();
        for (g, name) in group_by.iter().enumerate() {
            obj.insert(name.clone(), key_cells[g].clone());
        }
        for (s, spec) in computes.iter().enumerate() {
            let st = &states[s];
            let v = match spec.func {
                AggFunc::Count => serde_json::json!(st.count),
                AggFunc::Sum => serde_json::json!(st.sum),
                AggFunc::Avg => {
                    if st.count == 0 {
                        serde_json::Value::Null
                    } else {
                        serde_json::json!(st.sum / st.count as f64)
                    }
                }
                AggFunc::Min => st
                    .min_s
                    .as_ref()
                    .map(|s| serde_json::json!(s))
                    .or(st.min_f.map(|f| serde_json::json!(f)))
                    .unwrap_or(serde_json::Value::Null),
                AggFunc::Max => st
                    .max_s
                    .as_ref()
                    .map(|s| serde_json::json!(s))
                    .or(st.max_f.map(|f| serde_json::json!(f)))
                    .unwrap_or(serde_json::Value::Null),
            };
            obj.insert(spec.output_key(), v);
        }
        out.push(serde_json::Value::Object(obj));
    }
    Ok(QueryOutput {
        rows: serde_json::Value::Array(out),
        taint: store_taint(store),
        stats,
    })
}

/// Equi-⋈ — `associate` (§108.d, D108.5). Hash join on ONE shared
/// column (`using`), equality keys only. Output rows are flat: left
/// columns by name; right columns by name, prefixed `<right>_` on
/// collision (the join key appears once, from the left). NULL keys
/// never join (SQL semantics).
pub fn associate_query(
    left: &DataspaceStore,
    right: &DataspaceStore,
    using: &str,
) -> Result<QueryOutput, String> {
    let li = left.column_index(using).ok_or_else(|| {
        format!("`using {using}` — not a column of the left dataspace `{}`", left.name)
    })?;
    let ri = right.column_index(using).ok_or_else(|| {
        format!("`using {using}` — not a column of the right dataspace `{}`", right.name)
    })?;
    if left.schema()[li].1 != right.schema()[ri].1 {
        return Err(format!(
            "`using {using}` joins a {} column against a {} column (refusal, not coercion)",
            left.schema()[li].1.canonical_name(),
            right.schema()[ri].1.canonical_name()
        ));
    }
    let mut stats = QueryStats {
        batches_total: left.batches().len() + right.batches().len(),
        ..Default::default()
    };
    // Build side: hash the RIGHT store by canonical key string.
    use std::collections::HashMap as Map;
    let mut build: Map<String, Vec<(usize, usize)>> = Map::new(); // key → (batch, row)
    for (b, batch) in right.batches().iter().enumerate() {
        for i in 0..batch.len() {
            stats.rows_scanned += 1;
            let cell = cell_at(batch.column(ri).unwrap(), i);
            if matches!(cell, CellValue::Null) {
                continue;
            }
            build
                .entry(cell_to_json(&cell).to_string())
                .or_default()
                .push((b, i));
        }
    }
    let left_names: Vec<&str> = left.schema().iter().map(|(n, _)| n.as_str()).collect();
    let mut out: Vec<serde_json::Value> = Vec::new();
    for lbatch in left.batches() {
        for i in 0..lbatch.len() {
            stats.rows_scanned += 1;
            let key_cell = cell_at(lbatch.column(li).unwrap(), i);
            if matches!(key_cell, CellValue::Null) {
                continue;
            }
            let Some(matches) = build.get(&cell_to_json(&key_cell).to_string()) else {
                continue;
            };
            for &(rb, rr) in matches {
                stats.rows_matched += 1;
                let mut obj = serde_json::Map::new();
                for (c, (name, _)) in left.schema().iter().enumerate() {
                    obj.insert(
                        name.clone(),
                        cell_to_json(&cell_at(lbatch.column(c).unwrap(), i)),
                    );
                }
                let rbatch = &right.batches()[rb];
                for (c, (name, _)) in right.schema().iter().enumerate() {
                    if c == ri {
                        continue; // the join key appears once, from the left
                    }
                    let out_name = if left_names.contains(&name.as_str()) {
                        format!("{}_{name}", right.name)
                    } else {
                        name.clone()
                    };
                    obj.insert(
                        out_name,
                        cell_to_json(&cell_at(rbatch.column(c).unwrap(), rr)),
                    );
                }
                out.push(serde_json::Value::Object(obj));
            }
        }
    }
    Ok(QueryOutput {
        rows: serde_json::Value::Array(out),
        taint: taint_meet(store_taint(left), store_taint(right)),
        stats,
    })
}

/// Deterministic profile — `explore` (§108.d). Schema + row/null counts
/// + per-column zone ranges. NO row data: a profile describes shape,
/// it never samples content.
pub fn explore_profile(store: &DataspaceStore) -> QueryOutput {
    let schema = store.schema();
    let mut columns: Vec<serde_json::Value> = Vec::new();
    for (c, (name, ty)) in schema.iter().enumerate() {
        let mut nulls = 0usize;
        let mut mins: Vec<serde_json::Value> = Vec::new();
        let mut maxs: Vec<serde_json::Value> = Vec::new();
        for batch in store.batches() {
            if let Some(zm) = batch.zone_map(c) {
                nulls += zm.null_count;
                match &zm.stats {
                    ZoneStats::Int { min, max } => {
                        if *ty == ColumnType::Timestamp {
                            mins.push(serde_json::json!(micros_to_rfc3339(*min)));
                            maxs.push(serde_json::json!(micros_to_rfc3339(*max)));
                        } else {
                            mins.push(serde_json::json!(min));
                            maxs.push(serde_json::json!(max));
                        }
                    }
                    ZoneStats::Float { min, max } => {
                        mins.push(serde_json::json!(min));
                        maxs.push(serde_json::json!(max));
                    }
                    // A Text zone boundary IS row content (an email, a
                    // name — §104's no-PII discipline): a profile
                    // describes shape, so Text ranges are SUPPRESSED.
                    ZoneStats::Text { .. } => {}
                    ZoneStats::Bool { .. } | ZoneStats::None => {}
                }
            }
        }
        columns.push(serde_json::json!({
            "name": name,
            "type": ty.canonical_name(),
            "nulls": nulls,
            "min": mins.iter().min_by(cmp_json).cloned().unwrap_or(serde_json::Value::Null),
            "max": maxs.iter().max_by(cmp_json).cloned().unwrap_or(serde_json::Value::Null),
        }));
    }
    let taint = store_taint(store);
    QueryOutput {
        rows: serde_json::json!({
            "dataspace": store.name,
            "rows": store.row_count(),
            "batches": store.batches().len(),
            "resident_bytes": store.resident_bytes(),
            "taint": taint_str(taint),
            "columns": columns,
        }),
        taint,
        stats: QueryStats {
            batches_total: store.batches().len(),
            ..Default::default()
        },
    }
}

fn cmp_json(a: &&serde_json::Value, b: &&serde_json::Value) -> std::cmp::Ordering {
    match (a, b) {
        (serde_json::Value::Number(x), serde_json::Value::Number(y)) => x
            .as_f64()
            .partial_cmp(&y.as_f64())
            .unwrap_or(std::cmp::Ordering::Equal),
        (serde_json::Value::String(x), serde_json::Value::String(y)) => x.cmp(y),
        _ => std::cmp::Ordering::Equal,
    }
}

pub fn taint_str(t: EpistemicTaint) -> &'static str {
    match t {
        EpistemicTaint::Untrusted => "untrusted",
        EpistemicTaint::SchemaValidated => "schema_validated",
        EpistemicTaint::Elevated => "elevated",
    }
}

// ─────────────────────────────────────────────────────────────────────
//  Unit tests
// ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn provenance() -> BatchProvenance {
        BatchProvenance {
            source: "unit-test".into(),
            source_sha256: "0".repeat(64),
            ingested_at: "2026-07-12T00:00:00Z".into(),
            taint: EpistemicTaint::Untrusted,
        }
    }

    #[test]
    fn builder_roundtrips_every_type_with_nulls() {
        let mut ints = ColumnBuilder::new(ColumnType::Int);
        ints.push_int(7).unwrap();
        ints.push_null();
        ints.push_int(-3).unwrap();
        let ints = ints.finish();
        assert_eq!(ints.len(), 3);
        assert_eq!(ints.get_int(0), Some(7));
        assert_eq!(ints.get_int(1), None, "null is structural, no sentinel");
        assert_eq!(ints.get_int(2), Some(-3));
        assert_eq!(ints.null_count(), 1);

        let mut texts = ColumnBuilder::new(ColumnType::Text);
        texts.push_text("hola").unwrap();
        texts.push_null();
        texts.push_text("").unwrap(); // empty string ≠ null
        let texts = texts.finish();
        assert_eq!(texts.get_text(0), Some("hola"));
        assert_eq!(texts.get_text(1), None);
        assert_eq!(texts.get_text(2), Some(""), "empty string is present");

        let mut bools = ColumnBuilder::new(ColumnType::Bool);
        for i in 0..10 {
            bools.push_bool(i % 3 == 0).unwrap();
        }
        let bools = bools.finish();
        assert_eq!(bools.get_bool(0), Some(true));
        assert_eq!(bools.get_bool(1), Some(false));
        assert_eq!(bools.get_bool(9), Some(true));

        let mut floats = ColumnBuilder::new(ColumnType::Float);
        floats.push_float(1.5).unwrap();
        floats.push_float(-0.25).unwrap();
        let floats = floats.finish();
        assert_eq!(floats.get_float(1), Some(-0.25));
    }

    #[test]
    fn builder_refuses_a_type_mismatch() {
        // D108.7 — silent coercion is data laundering.
        let mut b = ColumnBuilder::new(ColumnType::Int);
        let err = b.push_text("42").unwrap_err();
        assert!(err.contains("type mismatch"), "{err}");
    }

    #[test]
    fn validated_refuses_broken_invariants() {
        // Bitmap length.
        assert!(ColumnArray::validated(9, vec![0u8; 1], ColumnData::Int(vec![0; 9])).is_err());
        // Fixed-width length.
        assert!(ColumnArray::validated(3, vec![0u8; 1], ColumnData::Int(vec![0; 2])).is_err());
        // Offsets must start at 0.
        assert!(ColumnArray::validated(
            1,
            vec![1u8],
            ColumnData::Text {
                offsets: vec![1, 2],
                bytes: vec![b'a', b'b'],
            }
        )
        .is_err());
        // Offsets monotone.
        assert!(ColumnArray::validated(
            2,
            vec![3u8],
            ColumnData::Text {
                offsets: vec![0, 5, 2],
                bytes: vec![0; 2],
            }
        )
        .is_err());
        // Final offset ≡ byte length.
        assert!(ColumnArray::validated(
            1,
            vec![1u8],
            ColumnData::Text {
                offsets: vec![0, 3],
                bytes: vec![b'a'],
            }
        )
        .is_err());
    }

    #[test]
    fn record_batch_refuses_schema_and_length_violations() {
        let schema = vec![
            ("a".to_string(), ColumnType::Int),
            ("b".to_string(), ColumnType::Text),
        ];
        let mut a = ColumnBuilder::new(ColumnType::Int);
        a.push_int(1).unwrap();
        // Wrong arity.
        assert!(RecordBatch::new(&schema, vec![a.finish()], provenance()).is_err());

        // Wrong type in slot b.
        let mut a = ColumnBuilder::new(ColumnType::Int);
        a.push_int(1).unwrap();
        let mut not_text = ColumnBuilder::new(ColumnType::Float);
        not_text.push_float(0.5).unwrap();
        assert!(
            RecordBatch::new(&schema, vec![a.finish(), not_text.finish()], provenance()).is_err()
        );

        // Ragged lengths.
        let mut a = ColumnBuilder::new(ColumnType::Int);
        a.push_int(1).unwrap();
        a.push_int(2).unwrap();
        let mut b = ColumnBuilder::new(ColumnType::Text);
        b.push_text("only-one").unwrap();
        let err =
            RecordBatch::new(&schema, vec![a.finish(), b.finish()], provenance()).unwrap_err();
        assert!(err.contains("ragged"), "{err}");
    }

    #[test]
    fn zone_maps_are_computed_at_construction() {
        let schema = vec![
            ("n".to_string(), ColumnType::Int),
            ("name".to_string(), ColumnType::Text),
        ];
        let mut n = ColumnBuilder::new(ColumnType::Int);
        n.push_int(10).unwrap();
        n.push_null();
        n.push_int(-4).unwrap();
        n.push_int(7).unwrap();
        let mut name = ColumnBuilder::new(ColumnType::Text);
        name.push_text("beta").unwrap();
        name.push_text("alpha").unwrap();
        name.push_text("gamma").unwrap();
        name.push_null();
        let batch =
            RecordBatch::new(&schema, vec![n.finish(), name.finish()], provenance()).unwrap();

        let zm = batch.zone_map(0).unwrap();
        assert_eq!(zm.null_count, 1);
        assert_eq!(zm.stats, ZoneStats::Int { min: -4, max: 10 });

        let zm = batch.zone_map(1).unwrap();
        assert_eq!(
            zm.stats,
            ZoneStats::Text {
                min: "alpha".into(),
                max: "gamma".into()
            }
        );
    }

    #[test]
    fn all_null_column_yields_zone_none() {
        let schema = vec![("x".to_string(), ColumnType::Float)];
        let mut x = ColumnBuilder::new(ColumnType::Float);
        x.push_null();
        x.push_null();
        let batch = RecordBatch::new(&schema, vec![x.finish()], provenance()).unwrap();
        assert_eq!(batch.zone_map(0).unwrap().stats, ZoneStats::None);
        assert_eq!(batch.zone_map(0).unwrap().null_count, 2);
    }

    #[test]
    fn nan_poisons_the_float_zone_to_none_never_unsound() {
        let schema = vec![("x".to_string(), ColumnType::Float)];
        let mut x = ColumnBuilder::new(ColumnType::Float);
        x.push_float(1.0).unwrap();
        x.push_float(f64::NAN).unwrap();
        let batch = RecordBatch::new(&schema, vec![x.finish()], provenance()).unwrap();
        // A NaN cannot be bounded by an interval — the zone degrades to
        // None (always scan) rather than risk an unsound prune.
        assert_eq!(batch.zone_map(0).unwrap().stats, ZoneStats::None);
    }

    #[test]
    fn taint_meet_is_the_lattice_min() {
        use EpistemicTaint::*;
        assert_eq!(taint_meet(Untrusted, Elevated), Untrusted);
        assert_eq!(taint_meet(Elevated, SchemaValidated), SchemaValidated);
        assert_eq!(taint_meet(Elevated, Elevated), Elevated);
    }

    fn ir_spec(name: &str, cols: &[(&str, &str)]) -> axon_frontend::ir_nodes::IRDataspace {
        axon_frontend::ir_nodes::IRDataspace {
            node_type: "dataspace",
            source_line: 1,
            source_column: 1,
            name: name.to_string(),
            columns: cols
                .iter()
                .map(|(n, t)| axon_frontend::ir_nodes::IRDataspaceColumn {
                    name: n.to_string(),
                    column_type: t.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn engine_instantiates_from_ir_specs() {
        let engine = DataspaceEngine::from_ir(&[
            ir_spec("Leads", &[("email", "Text"), ("score", "Float")]),
            ir_spec("Events", &[("at", "Timestamp"), ("payload", "Json")]),
        ])
        .unwrap();
        assert_eq!(engine.len(), 2);
        let leads = engine.store("Leads").unwrap();
        assert_eq!(leads.schema().len(), 2);
        assert_eq!(leads.column_index("score"), Some(1));
        assert_eq!(leads.row_count(), 0);
    }

    #[test]
    fn engine_refuses_a_stale_ir_with_unknown_type() {
        // Fail CLOSED on the whole deploy — never a partial engine.
        let err = DataspaceEngine::from_ir(&[ir_spec("X", &[("a", "Decimal")])]).unwrap_err();
        assert!(err.contains("axon-T928"), "names the bypassed law: {err}");
    }

    #[test]
    fn store_append_and_scan_via_batches() {
        let mut engine =
            DataspaceEngine::from_ir(&[ir_spec("M", &[("k", "Text"), ("v", "Int")])]).unwrap();
        let store = engine.store_mut("M").unwrap();
        let schema: Vec<(String, ColumnType)> = store.schema().to_vec();

        let mut k = ColumnBuilder::new(ColumnType::Text);
        k.push_text("a").unwrap();
        k.push_text("b").unwrap();
        let mut v = ColumnBuilder::new(ColumnType::Int);
        v.push_int(1).unwrap();
        v.push_int(2).unwrap();
        let batch = RecordBatch::new(&schema, vec![k.finish(), v.finish()], provenance()).unwrap();
        store.append(batch).unwrap();
        assert_eq!(store.row_count(), 2);
        assert!(store.resident_bytes() > 0);

        // Append-only (D108.2): a second batch accumulates; nothing mutates.
        let mut k = ColumnBuilder::new(ColumnType::Text);
        k.push_text("c").unwrap();
        let mut v = ColumnBuilder::new(ColumnType::Int);
        v.push_int(3).unwrap();
        let b2 = RecordBatch::new(&schema, vec![k.finish(), v.finish()], provenance()).unwrap();
        store.append(b2).unwrap();
        assert_eq!(store.batches().len(), 2);
        assert_eq!(store.row_count(), 3);
    }

    // ── §108.c — the governed loaders ────────────────────────────────

    fn lead_schema() -> Vec<(String, ColumnType)> {
        vec![
            ("email".to_string(), ColumnType::Text),
            ("score".to_string(), ColumnType::Float),
            ("visits".to_string(), ColumnType::Int),
        ]
    }

    #[test]
    fn ingest_csv_types_rows_against_the_schema() {
        let csv = "email,score,visits,extra\na@x.com,0.9,3,zz\nb@x.com,,7,zz\n";
        let batch = ingest_bytes(
            &lead_schema(),
            IngestFormat::Csv,
            csv.as_bytes(),
            &IngestLimits::default(),
            provenance(),
        )
        .unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch.column(0).unwrap().get_text(1), Some("b@x.com"));
        assert_eq!(
            batch.column(1).unwrap().get_float(1),
            None,
            "empty CSV field = structural null"
        );
        assert_eq!(batch.column(2).unwrap().get_int(1), Some(7));
        // Extra source column projected away; provenance stamped.
        assert_eq!(batch.provenance().taint, EpistemicTaint::Untrusted);
    }

    #[test]
    fn ingest_refuses_bytes_bound_before_any_parse() {
        // §100 — the refusal must be the BOUNDS error, not a parse error,
        // even though the payload is also malformed CSV.
        let garbage = vec![b'"'; 4096]; // unclosed quote AND oversized
        let err = ingest_bytes(
            &lead_schema(),
            IngestFormat::Csv,
            &garbage,
            &IngestLimits {
                max_bytes: 1024,
                max_rows: 10,
            },
            provenance(),
        )
        .unwrap_err();
        assert!(
            err.contains("BEFORE parse"),
            "bounds must precede parsing: {err}"
        );
    }

    #[test]
    fn ingest_refuses_rows_bound() {
        let mut csv = String::from("email,score,visits\n");
        for i in 0..5 {
            csv.push_str(&format!("u{i}@x.com,0.5,1\n"));
        }
        let err = ingest_bytes(
            &lead_schema(),
            IngestFormat::Csv,
            csv.as_bytes(),
            &IngestLimits {
                max_bytes: 1024 * 1024,
                max_rows: 3,
            },
            provenance(),
        )
        .unwrap_err();
        assert!(err.contains("5 rows"), "{err}");
    }

    #[test]
    fn ingest_type_mismatch_refuses_naming_row_and_column() {
        // D108.7 — refusal, not coercion; the error is actionable.
        let csv = "email,score,visits\na@x.com,not_a_number,3\n";
        let err = ingest_bytes(
            &lead_schema(),
            IngestFormat::Csv,
            csv.as_bytes(),
            &IngestLimits::default(),
            provenance(),
        )
        .unwrap_err();
        assert!(err.contains("row 1"), "{err}");
        assert!(err.contains("`score`"), "{err}");
    }

    #[test]
    fn ingest_csv_missing_schema_column_in_header_refuses() {
        let csv = "email,visits\na@x.com,3\n"; // no `score`
        let err = ingest_bytes(
            &lead_schema(),
            IngestFormat::Csv,
            csv.as_bytes(),
            &IngestLimits::default(),
            provenance(),
        )
        .unwrap_err();
        assert!(err.contains("`score`"), "null-filling is laundering: {err}");
    }

    #[test]
    fn ingest_csv_quoted_fields_roundtrip() {
        let csv = "email,score,visits\n\"a,with,commas@x.com\",0.5,1\n\"say \"\"hi\"\"\",0.1,2\n";
        let schema = vec![
            ("email".to_string(), ColumnType::Text),
            ("score".to_string(), ColumnType::Float),
            ("visits".to_string(), ColumnType::Int),
        ];
        let batch = ingest_bytes(
            &schema,
            IngestFormat::Csv,
            csv.as_bytes(),
            &IngestLimits::default(),
            provenance(),
        )
        .unwrap();
        assert_eq!(
            batch.column(0).unwrap().get_text(0),
            Some("a,with,commas@x.com")
        );
        assert_eq!(batch.column(0).unwrap().get_text(1), Some("say \"hi\""));
    }

    #[test]
    fn ingest_json_rows_with_nulls_timestamps_and_json_columns() {
        let schema = vec![
            ("who".to_string(), ColumnType::Text),
            ("at".to_string(), ColumnType::Timestamp),
            ("meta".to_string(), ColumnType::Json),
        ];
        let src = r#"[
            {"who":"ana","at":"2026-07-12T10:00:00Z","meta":{"k":1}},
            {"who":null,"at":null}
        ]"#;
        let batch = ingest_bytes(
            &schema,
            IngestFormat::Json,
            src.as_bytes(),
            &IngestLimits::default(),
            provenance(),
        )
        .unwrap();
        assert_eq!(batch.len(), 2);
        assert!(batch.column(1).unwrap().get_int(0).unwrap() > 0);
        assert_eq!(batch.column(1).unwrap().get_int(1), None, "null + absent = null");
        assert_eq!(
            batch.column(2).unwrap().get_text(0),
            Some(r#"{"k":1}"#),
            "Json column keeps ONE canonical compact byte form"
        );
        assert_eq!(batch.column(2).unwrap().get_bytes(1), None);
    }

    #[test]
    fn ingest_json_int_column_refuses_fractional_number() {
        let schema = vec![("n".to_string(), ColumnType::Int)];
        let err = ingest_bytes(
            &schema,
            IngestFormat::Json,
            br#"[{"n": 1.5}]"#,
            &IngestLimits::default(),
            provenance(),
        )
        .unwrap_err();
        assert!(err.contains("expected Int"), "{err}");
    }

    #[test]
    fn ingest_json_non_array_root_refuses() {
        let schema = vec![("n".to_string(), ColumnType::Int)];
        let err = ingest_bytes(
            &schema,
            IngestFormat::Json,
            br#"{"n": 1}"#,
            &IngestLimits::default(),
            provenance(),
        )
        .unwrap_err();
        assert!(err.contains("ARRAY"), "{err}");
    }

    // ── §108.d — the lazy relational query engine ────────────────────

    fn populated_store() -> DataspaceStore {
        let mut engine = DataspaceEngine::from_ir(&[ir_spec(
            "Leads",
            &[("email", "Text"), ("score", "Float"), ("visits", "Int"), ("region", "Text")],
        )])
        .unwrap();
        {
            let store = engine.store_mut("Leads").unwrap();
            let schema = store.schema().to_vec();
            // Batch 1: scores 0.1..0.4 (low), region south.
            let csv1 = "email,score,visits,region\na@x.com,0.1,1,south\nb@x.com,0.4,2,south\n";
            let b1 = ingest_bytes(&schema, IngestFormat::Csv, csv1.as_bytes(), &IngestLimits::default(), provenance()).unwrap();
            store.append(b1).unwrap();
            // Batch 2: scores 0.7..0.9 (high), region north; one null score.
            let csv2 = "email,score,visits,region\nc@x.com,0.9,7,north\nd@x.com,0.7,3,north\ne@x.com,,5,north\n";
            let b2 = ingest_bytes(&schema, IngestFormat::Csv, csv2.as_bytes(), &IngestLimits::default(), provenance()).unwrap();
            store.append(b2).unwrap();
        }
        engine.stores.remove("Leads").unwrap()
    }

    #[test]
    fn focus_selects_and_projects_with_pruning() {
        let store = populated_store();
        let out = focus_query(
            &store,
            "score >= 0.6",
            &["email".to_string()],
            &std::collections::HashMap::new(),
        )
        .unwrap();
        let rows = out.rows.as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["email"], "c@x.com");
        assert!(rows[0].get("score").is_none(), "projection drops unselected");
        // Batch 1's zone is [0.1, 0.4] — provably below 0.6 => pruned.
        assert_eq!(out.stats.batches_pruned, 1, "zone maps prune the low batch");
        assert_eq!(out.stats.rows_scanned, 3, "only batch 2 was scanned");
        assert_eq!(out.taint, EpistemicTaint::Untrusted, "the meet survives sigma/pi");
    }

    #[test]
    fn focus_sql_precedence_and_binds_tighter_than_or() {
        let store = populated_store();
        // (region = north AND score >= 0.8) OR visits = 1 -> c@x.com + a@x.com.
        let out = focus_query(
            &store,
            "region = 'north' AND score >= 0.8 OR visits = 1",
            &["email".to_string()],
            &std::collections::HashMap::new(),
        )
        .unwrap();
        let emails: Vec<&str> = out
            .rows
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["email"].as_str().unwrap())
            .collect();
        assert_eq!(emails, vec!["a@x.com", "c@x.com"]);
    }

    #[test]
    fn focus_unknown_where_column_fails_closed() {
        let store = populated_store();
        let err = focus_query(&store, "ghost = 1", &[], &std::collections::HashMap::new())
            .unwrap_err();
        assert!(err.contains("`ghost`"), "{err}");
        assert!(err.contains("declared:"), "actionable: {err}");
    }

    #[test]
    fn zone_pruning_is_sound_at_the_boundaries() {
        // Plan 5.3 adversarial gate: for every operator, a batch whose
        // zone boundary EQUALS the literal must never be pruned when a
        // matching row exists. Soundness = zero false prunes.
        let store = populated_store();
        // Batch 1 zone for score = [0.1, 0.4]; batch 2 = [0.7, 0.9].
        for (clause, expect) in [
            ("score = 0.4", 1),
            ("score >= 0.9", 1),
            ("score <= 0.1", 1),
            ("score > 0.9", 0),
            ("score < 0.1", 0),
        ] {
            let out = focus_query(&store, clause, &[], &std::collections::HashMap::new()).unwrap();
            assert_eq!(
                out.rows.as_array().unwrap().len(),
                expect,
                "`{clause}` — a skipped batch must PROVABLY contain no match"
            );
        }
    }

    #[test]
    fn null_semantics_match_sql() {
        let store = populated_store();
        // `score = NULL` -> is-null (e@x.com).
        let out = focus_query(&store, "score = NULL", &["email".to_string()], &Default::default()).unwrap();
        assert_eq!(out.rows.as_array().unwrap().len(), 1);
        assert_eq!(out.rows[0]["email"], "e@x.com");
        // Ordering against a literal never matches a NULL cell.
        let out = focus_query(&store, "score >= 0.0", &[], &Default::default()).unwrap();
        assert_eq!(out.rows.as_array().unwrap().len(), 4, "the null row matches nothing ordered");
    }

    #[test]
    fn aggregate_groups_and_computes_deterministically() {
        let store = populated_store();
        let computes = vec![
            AggregateSpec::parse("count").unwrap(),
            AggregateSpec::parse("avg(score)").unwrap(),
            AggregateSpec::parse("sum(visits)").unwrap(),
        ];
        let out = aggregate_query(&store, "", &["region".to_string()], &computes, &Default::default()).unwrap();
        let rows = out.rows.as_array().unwrap();
        assert_eq!(rows.len(), 2, "two regions");
        // BTreeMap ordering => deterministic: north before south.
        assert_eq!(rows[0]["region"], "north");
        assert_eq!(rows[0]["count"], 3);
        assert_eq!(rows[0]["sum_visits"], 15.0);
        // avg(score) over north skips the NULL: (0.9 + 0.7) / 2 = 0.8.
        assert!((rows[0]["avg_score"].as_f64().unwrap() - 0.8).abs() < 1e-9, "nulls are SKIPPED, not zero-counted");
        assert_eq!(rows[1]["region"], "south");
        assert_eq!(rows[1]["count"], 2);
    }

    #[test]
    fn aggregate_type_discipline_refuses_sum_over_text() {
        let store = populated_store();
        let err = aggregate_query(
            &store,
            "",
            &[],
            &[AggregateSpec::parse("sum(email)").unwrap()],
            &Default::default(),
        )
        .unwrap_err();
        assert!(err.contains("Int/Float"), "{err}");
    }

    #[test]
    fn aggregate_parse_catalog_is_closed() {
        assert!(AggregateSpec::parse("median(x)").unwrap_err().contains("closed catalog"));
        assert!(AggregateSpec::parse("sum").unwrap_err().contains("requires a column"));
    }

    #[test]
    fn associate_hash_joins_on_the_shared_key() {
        let mut engine = DataspaceEngine::from_ir(&[
            ir_spec("People", &[("id", "Int"), ("name", "Text")]),
            ir_spec("Orders", &[("id", "Int"), ("total", "Float"), ("name", "Text")]),
        ])
        .unwrap();
        {
            let people = engine.store_mut("People").unwrap();
            let schema = people.schema().to_vec();
            let b = ingest_bytes(&schema, IngestFormat::Json,
                br#"[{"id":1,"name":"ana"},{"id":2,"name":"leo"},{"id":null,"name":"ghost"}]"#,
                &IngestLimits::default(), provenance()).unwrap();
            people.append(b).unwrap();
        }
        {
            let orders = engine.store_mut("Orders").unwrap();
            let schema = orders.schema().to_vec();
            let b = ingest_bytes(&schema, IngestFormat::Json,
                br#"[{"id":1,"total":9.5,"name":"o-1"},{"id":1,"total":3.0,"name":"o-2"},{"id":3,"total":1.0,"name":"o-x"}]"#,
                &IngestLimits::default(), provenance()).unwrap();
            orders.append(b).unwrap();
        }
        let out = associate_query(
            engine.store("People").unwrap(),
            engine.store("Orders").unwrap(),
            "id",
        )
        .unwrap();
        let rows = out.rows.as_array().unwrap();
        assert_eq!(rows.len(), 2, "ana x2 orders; leo x0; NULL key never joins; id=3 unmatched");
        assert_eq!(rows[0]["name"], "ana", "left column by name");
        assert_eq!(rows[0]["Orders_name"], "o-1", "right collision prefixed");
        assert_eq!(rows[0]["total"], 9.5, "non-colliding right column by name");
        assert_eq!(out.taint, EpistemicTaint::Untrusted, "the meet spans BOTH stores");
    }

    #[test]
    fn associate_refuses_key_type_mismatch() {
        let engine = DataspaceEngine::from_ir(&[
            ir_spec("A", &[("k", "Int")]),
            ir_spec("B", &[("k", "Text")]),
        ])
        .unwrap();
        let err = associate_query(engine.store("A").unwrap(), engine.store("B").unwrap(), "k")
            .unwrap_err();
        assert!(err.contains("refusal, not coercion"), "{err}");
    }

    #[test]
    fn explore_profiles_shape_never_content() {
        let store = populated_store();
        let out = explore_profile(&store);
        let p = &out.rows;
        assert_eq!(p["rows"], 5);
        assert_eq!(p["batches"], 2);
        assert_eq!(p["taint"], "untrusted");
        let cols = p["columns"].as_array().unwrap();
        assert_eq!(cols.len(), 4);
        let score = cols.iter().find(|c| c["name"] == "score").unwrap();
        assert_eq!(score["nulls"], 1);
        assert_eq!(score["min"], 0.1);
        assert_eq!(score["max"], 0.9);
        // No row content anywhere in the profile.
        assert!(!out.rows.to_string().contains("a@x.com"), "shape, never content");
    }

    // ── §108.x — the wire format (durable snapshots, D108.8) ─────────

    #[test]
    fn wire_roundtrip_is_lossless_across_all_types() {
        let store = populated_store();
        let schema = store.schema().to_vec();
        for batch in store.batches() {
            let wire = batch.to_wire();
            // JSON round-trip too (the BlobStore carries JSON bytes).
            let json = serde_json::to_string(&wire).unwrap();
            let wire2: WireBatch = serde_json::from_str(&json).unwrap();
            let back = RecordBatch::from_wire(&schema, &wire2).unwrap();
            assert_eq!(back.len(), batch.len());
            for c in 0..schema.len() {
                for i in 0..batch.len() {
                    assert_eq!(
                        cell_at(back.column(c).unwrap(), i),
                        cell_at(batch.column(c).unwrap(), i),
                        "cell ({c},{i}) must survive the wire"
                    );
                }
            }
            assert_eq!(back.provenance().source_sha256, batch.provenance().source_sha256);
            assert_eq!(back.provenance().taint, batch.provenance().taint);
        }
    }

    #[test]
    fn wire_refuses_a_tampered_or_schema_drifted_snapshot() {
        let store = populated_store();
        let schema = store.schema().to_vec();
        let mut wire = store.batches()[0].to_wire();
        // Truncated buffer → the §5.1 invariants refuse it whole.
        wire.columns[1].data_b64 = b64_encode(&[0u8; 4]); // score: 4 bytes ≠ n×8
        assert!(RecordBatch::from_wire(&schema, &wire).is_err());
        // Schema drift → refused.
        let wire = store.batches()[0].to_wire();
        let wrong = vec![("email".to_string(), ColumnType::Int)];
        assert!(RecordBatch::from_wire(&wrong, &wire).is_err());
        // Unknown taint → refused.
        let mut wire = store.batches()[0].to_wire();
        wire.taint = "trusted_bro".into();
        assert!(RecordBatch::from_wire(&schema, &wire).is_err());
    }

    #[test]
    fn b64_roundtrips_arbitrary_bytes() {
        for len in [0usize, 1, 2, 3, 4, 63, 64, 65] {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 % 251) as u8).collect();
            assert_eq!(b64_decode(&b64_encode(&bytes)).unwrap(), bytes, "len {len}");
        }
        assert!(b64_decode("not!valid").is_err());
    }

    #[test]
    fn like_matcher_is_sql_like() {
        assert!(like_match("hello world", "hello%"));
        assert!(like_match("hello", "h_llo"));
        assert!(!like_match("hello", "h_"));
        assert!(like_match("a@x.com", "%@x.com"));
        assert!(!like_match("a@y.com", "%@x.com"));
    }

    #[test]
    fn store_append_refuses_schema_mismatch() {
        let mut engine = DataspaceEngine::from_ir(&[ir_spec("M", &[("v", "Int")])]).unwrap();
        // A batch built against a DIFFERENT schema shape.
        let other_schema = vec![("v".to_string(), ColumnType::Float)];
        let mut v = ColumnBuilder::new(ColumnType::Float);
        v.push_float(0.5).unwrap();
        let batch = RecordBatch::new(&other_schema, vec![v.finish()], provenance()).unwrap();
        let err = engine.store_mut("M").unwrap().append(batch).unwrap_err();
        assert!(err.contains("Int"), "{err}");
    }
}
