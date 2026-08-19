//! Lambda Data (ΛD) — Epistemic State Vector codec.
//!
//! Formal basis (paper_lambda_data.md):
//!
//!   ΛD: V → (V × O × C × T)
//!   ψ = ⟨T, V, E⟩  where  E = ⟨c, τ, ρ, δ⟩
//!
//! This module implements the **lossless binary codec** for ΛD state vectors.
//! Unlike JSON projection (π_JSON(ψ) = V, which discards T and E), the ΛD
//! binary format preserves the full epistemic tensor across serialization
//! boundaries.
//!
//! Invariants enforced at encode boundary:
//!   1. Ontological Rigidity:  T ∈ O ∧ T ≠ ⊥
//!   2. Singular Interpretation: V ∈ dom(T)  (deferred to runtime)
//!   3. Semantic Conservation: type preservation across transformations
//!   4. Epistemic Bounding: c ∈ [0,1] ∧ δ ∈ Δ
//!
//! Theorem 5.1 (Epistemic Degradation):
//!   For any composition f operating on ΛD inputs,
//!     c_out ≤ min(c_in₁, c_in₂, …, c_inₙ)
//!   Enforced at compose time.

use std::io::{self, Read};

// ── Magic bytes & version ───────────────────────────────────────────────────

/// File signature: "ΛD" in UTF-8 (0xCE 0x9B 0x44) + version byte.
const MAGIC: [u8; 3] = [0xCE, 0x9B, 0x44]; // "ΛD" as UTF-8
const FORMAT_VERSION: u8 = 1;

// ── Derivation enum ─────────────────────────────────────────────────────────

/// δ ∈ Δ = {raw, derived, inferred, aggregated, transformed}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Derivation {
    Raw = 0,
    Derived = 1,
    Inferred = 2,
    Aggregated = 3,
    Transformed = 4,
}

impl Derivation {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "raw" => Some(Derivation::Raw),
            "derived" => Some(Derivation::Derived),
            "inferred" => Some(Derivation::Inferred),
            "aggregated" => Some(Derivation::Aggregated),
            "transformed" => Some(Derivation::Transformed),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Derivation::Raw => "raw",
            Derivation::Derived => "derived",
            Derivation::Inferred => "inferred",
            Derivation::Aggregated => "aggregated",
            Derivation::Transformed => "transformed",
        }
    }

    fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Derivation::Raw),
            1 => Some(Derivation::Derived),
            2 => Some(Derivation::Inferred),
            3 => Some(Derivation::Aggregated),
            4 => Some(Derivation::Transformed),
            _ => None,
        }
    }
}

// ── Epistemic State Vector ──────────────────────────────────────────────────

/// ψ = ⟨T, V, E⟩ where E = ⟨c, τ, ρ, δ⟩
///
/// T — Ontological type tag (domain classification)
/// V — The value payload (opaque bytes, interpretation depends on T)
/// E — Epistemic tensor:
///     c — certainty scalar, c ∈ [0, 1]
///     τ — temporal validity frame [t_start, t_end]
///     ρ — provenance EntityRef (causal origin)
///     δ — derivation ∈ Δ
#[derive(Debug, Clone)]
pub struct LambdaData {
    pub name: String,
    pub ontology: String,             // T
    pub value: Vec<u8>,               // V (opaque payload)
    pub certainty: f64,               // c ∈ [0,1]
    pub temporal_frame_start: String, // τ_start
    pub temporal_frame_end: String,   // τ_end
    pub provenance: String,           // ρ
    pub derivation: Derivation,       // δ
}

// ── Codec errors ────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum LdError {
    /// Invariant violation at encode boundary.
    InvariantViolation(String),
    /// Binary format error during decode.
    DecodeError(String),
    /// IO error.
    Io(io::Error),
}

impl From<io::Error> for LdError {
    fn from(e: io::Error) -> Self {
        LdError::Io(e)
    }
}

impl std::fmt::Display for LdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LdError::InvariantViolation(msg) => write!(f, "ΛD invariant violation: {msg}"),
            LdError::DecodeError(msg) => write!(f, "ΛD decode error: {msg}"),
            LdError::Io(e) => write!(f, "ΛD I/O error: {e}"),
        }
    }
}

// ── Invariant validation ────────────────────────────────────────────────────

impl LambdaData {
    /// Validate all encode-boundary invariants.
    pub fn validate(&self) -> Result<(), LdError> {
        // Invariant 1 — Ontological Rigidity: T ∈ O ∧ T ≠ ⊥
        if self.ontology.is_empty() {
            return Err(LdError::InvariantViolation(format!(
                "Ontological Rigidity: '{}' has empty ontology (T = ⊥)",
                self.name
            )));
        }

        // Invariant 4 — Epistemic Bounding: c ∈ [0, 1]
        if self.certainty < 0.0 || self.certainty > 1.0 {
            return Err(LdError::InvariantViolation(format!(
                "Epistemic Bounding: certainty={} for '{}' (must be in [0, 1])",
                self.certainty, self.name
            )));
        }

        // Theorem 5.1 — Epistemic Degradation: only raw may carry c = 1.0
        if self.certainty == 1.0 && self.derivation != Derivation::Raw {
            return Err(LdError::InvariantViolation(format!(
                "Epistemic Degradation: '{}' has c=1.0 with δ={}, only raw may carry absolute certainty",
                self.name, self.derivation.as_str()
            )));
        }

        Ok(())
    }
}

// ── Binary format ───────────────────────────────────────────────────────────
//
// Layout (little-endian):
//   [3 bytes]  magic: 0xCE 0x9B 0x44 ("ΛD" UTF-8)
//   [1 byte]   version
//   [2+N]      name: u16 len + UTF-8 bytes
//   [2+N]      ontology: u16 len + UTF-8 bytes
//   [8 bytes]  certainty: f64
//   [2+N]      temporal_frame_start: u16 len + UTF-8 bytes
//   [2+N]      temporal_frame_end: u16 len + UTF-8 bytes
//   [2+N]      provenance: u16 len + UTF-8 bytes
//   [1 byte]   derivation: u8 enum tag
//   [4+N]      value: u32 len + raw bytes
//

/// Encode a ΛD state vector to binary. Validates invariants at boundary.
pub fn encode(ld: &LambdaData) -> Result<Vec<u8>, LdError> {
    ld.validate()?;

    let mut buf: Vec<u8> = Vec::new();

    // Header
    buf.extend_from_slice(&MAGIC);
    buf.push(FORMAT_VERSION);

    // Strings: name, ontology, temporal frames, provenance
    write_str(&mut buf, &ld.name)?;
    write_str(&mut buf, &ld.ontology)?;

    // Certainty (f64 LE)
    buf.extend_from_slice(&ld.certainty.to_le_bytes());

    // Temporal frame
    write_str(&mut buf, &ld.temporal_frame_start)?;
    write_str(&mut buf, &ld.temporal_frame_end)?;

    // Provenance
    write_str(&mut buf, &ld.provenance)?;

    // Derivation (single byte)
    buf.push(ld.derivation as u8);

    // Value payload (u32 length prefix)
    let vlen = ld.value.len() as u32;
    buf.extend_from_slice(&vlen.to_le_bytes());
    buf.extend_from_slice(&ld.value);

    Ok(buf)
}

/// Decode a ΛD state vector from binary. Validates invariants after decode.
pub fn decode(data: &[u8]) -> Result<LambdaData, LdError> {
    let mut cursor = io::Cursor::new(data);

    // Magic
    let mut magic = [0u8; 3];
    cursor
        .read_exact(&mut magic)
        .map_err(|_| LdError::DecodeError("truncated: missing magic bytes".into()))?;
    if magic != MAGIC {
        return Err(LdError::DecodeError(format!(
            "invalid magic: expected [CE 9B 44], got [{:02X} {:02X} {:02X}]",
            magic[0], magic[1], magic[2]
        )));
    }

    // Version
    let mut ver = [0u8; 1];
    cursor
        .read_exact(&mut ver)
        .map_err(|_| LdError::DecodeError("truncated: missing version byte".into()))?;
    if ver[0] != FORMAT_VERSION {
        return Err(LdError::DecodeError(format!(
            "unsupported version: {} (expected {})",
            ver[0], FORMAT_VERSION
        )));
    }

    // Fields
    let name = read_str(&mut cursor)?;
    let ontology = read_str(&mut cursor)?;

    let mut c_bytes = [0u8; 8];
    cursor
        .read_exact(&mut c_bytes)
        .map_err(|_| LdError::DecodeError("truncated: missing certainty".into()))?;
    let certainty = f64::from_le_bytes(c_bytes);

    let temporal_frame_start = read_str(&mut cursor)?;
    let temporal_frame_end = read_str(&mut cursor)?;
    let provenance = read_str(&mut cursor)?;

    let mut d_byte = [0u8; 1];
    cursor
        .read_exact(&mut d_byte)
        .map_err(|_| LdError::DecodeError("truncated: missing derivation".into()))?;
    let derivation = Derivation::from_byte(d_byte[0])
        .ok_or_else(|| LdError::DecodeError(format!("invalid derivation tag: {}", d_byte[0])))?;

    let mut vlen_bytes = [0u8; 4];
    cursor
        .read_exact(&mut vlen_bytes)
        .map_err(|_| LdError::DecodeError("truncated: missing value length".into()))?;
    let vlen = u32::from_le_bytes(vlen_bytes) as usize;
    let mut value = vec![0u8; vlen];
    cursor
        .read_exact(&mut value)
        .map_err(|_| LdError::DecodeError("truncated: value payload incomplete".into()))?;

    let ld = LambdaData {
        name,
        ontology,
        value,
        certainty,
        temporal_frame_start,
        temporal_frame_end,
        provenance,
        derivation,
    };

    // Validate after decode (invariants must hold on deserialized data)
    ld.validate()?;

    Ok(ld)
}

// ── Composition (Theorem 5.1) ───────────────────────────────────────────────

/// Compose two ΛD state vectors under Theorem 5.1 (Epistemic Degradation).
///
/// The composed ψ inherits:
///   c_out = min(c₁, c₂)           — certainty cannot increase
///   δ_out = max(δ₁, δ₂)           — derivation can only increase (raw < derived < inferred < aggregated < transformed)
///   τ_out = intersection(τ₁, τ₂)  — temporal frame narrows
///   ρ_out = "ρ₁ ∘ ρ₂"             — provenance chain concatenation
pub fn compose(
    a: &LambdaData,
    b: &LambdaData,
    result_name: &str,
    result_ontology: &str,
) -> Result<LambdaData, LdError> {
    // Theorem 5.1: c_out ≤ min(c_in₁, c_in₂)
    let c_out = a.certainty.min(b.certainty);

    // Derivation: max (most derived wins)
    let d_out = if (a.derivation as u8) >= (b.derivation as u8) {
        a.derivation
    } else {
        b.derivation
    };

    // Temporal frame: intersection (most restrictive)
    let tf_start = if a.temporal_frame_start >= b.temporal_frame_start {
        &a.temporal_frame_start
    } else {
        &b.temporal_frame_start
    };
    let tf_end = if a.temporal_frame_end.is_empty() {
        &b.temporal_frame_end
    } else if b.temporal_frame_end.is_empty() {
        &a.temporal_frame_end
    } else if a.temporal_frame_end <= b.temporal_frame_end {
        &a.temporal_frame_end
    } else {
        &b.temporal_frame_end
    };

    // Provenance: chain
    let prov = if a.provenance.is_empty() {
        b.provenance.clone()
    } else if b.provenance.is_empty() {
        a.provenance.clone()
    } else {
        format!("{} \u{2218} {}", a.provenance, b.provenance)
    };

    let composed = LambdaData {
        name: result_name.to_string(),
        ontology: result_ontology.to_string(),
        value: Vec::new(), // composed value is deferred to runtime
        certainty: c_out,
        temporal_frame_start: tf_start.clone(),
        temporal_frame_end: tf_end.clone(),
        provenance: prov,
        derivation: d_out,
    };

    composed.validate()?;
    Ok(composed)
}

/// v2.5.0 — tainted-overriding (founder refinement A). A
/// PROVENANCE member's declared `default_confidence` — an `extension`
/// member's ceiling (v2.5.0), or the built-in `epistemic:<level>` axis — is
/// a CEILING on the announced certainty, never a floor. When a value
/// with certainty `input_c` is annotated with such a member, the
/// announced certainty degrades to `min(ceiling, input_c)`.
///
/// This is the SAME Theorem 5.1 (Epistemic Degradation) rule [`compose`]
/// applies — `c_out = min(c₁, c₂)` — with the declared ceiling as one
/// operand. A doubtful input (`input_c < ceiling`) is therefore NEVER
/// laundered UP to the declared ceiling; certainty cannot increase.
///
/// Pure + total. Both operands are clamped to `[0,1]` defensively so a
/// malformed declared ceiling cannot push the result out of range.
///
/// **Scope (honest):** this is the mathematical rule, ready to wire.
/// Driving it from live step execution needs a path from a step's
/// effect-row provenance annotation to the runtime ψ-envelope of the
/// value it produces — today effect rows are static contract metadata,
/// not runtime certainty carriers, so that plumbing is a separate
/// feature. The rule here is the contract that plumbing will call.
pub fn apply_provenance_ceiling(input_c: f64, ceiling: f64) -> f64 {
    input_c.clamp(0.0, 1.0).min(ceiling.clamp(0.0, 1.0))
}

// ── JSON projection (lossy) ─────────────────────────────────────────────────

/// π_JSON(ψ) — lossy projection that discards epistemic tensor.
///
/// Returns a JSON object with all fields for inspection, but marks
/// the projection as lossy with ΔH > 0 (information entropy increase).
pub fn to_json(ld: &LambdaData) -> serde_json::Value {
    serde_json::json!({
        "_ld_version": FORMAT_VERSION,
        "_ld_lossy": true,
        "name": ld.name,
        "ontology": ld.ontology,
        "certainty": ld.certainty,
        "temporal_frame_start": ld.temporal_frame_start,
        "temporal_frame_end": ld.temporal_frame_end,
        "provenance": ld.provenance,
        "derivation": ld.derivation.as_str(),
        "value_bytes": ld.value.len(),
    })
}

/// Create a LambdaData from IR fields (bridge from compiler to runtime).
pub fn from_ir(
    name: &str,
    ontology: &str,
    certainty: f64,
    temporal_frame_start: &str,
    temporal_frame_end: &str,
    provenance: &str,
    derivation: &str,
) -> Result<LambdaData, LdError> {
    let d = Derivation::from_str(derivation)
        .ok_or_else(|| LdError::InvariantViolation(format!("unknown derivation '{derivation}'")))?;

    let ld = LambdaData {
        name: name.to_string(),
        ontology: ontology.to_string(),
        value: Vec::new(),
        certainty,
        temporal_frame_start: temporal_frame_start.to_string(),
        temporal_frame_end: temporal_frame_end.to_string(),
        provenance: provenance.to_string(),
        derivation: d,
    };

    ld.validate()?;
    Ok(ld)
}

// ── Wire helpers ────────────────────────────────────────────────────────────

fn write_str(buf: &mut Vec<u8>, s: &str) -> Result<(), LdError> {
    let bytes = s.as_bytes();
    if bytes.len() > u16::MAX as usize {
        return Err(LdError::InvariantViolation(format!(
            "string too long for ΛD format: {} bytes (max {})",
            bytes.len(),
            u16::MAX
        )));
    }
    buf.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    buf.extend_from_slice(bytes);
    Ok(())
}

fn read_str(cursor: &mut io::Cursor<&[u8]>) -> Result<String, LdError> {
    let mut len_bytes = [0u8; 2];
    cursor
        .read_exact(&mut len_bytes)
        .map_err(|_| LdError::DecodeError("truncated: missing string length".into()))?;
    let len = u16::from_le_bytes(len_bytes) as usize;
    let mut buf = vec![0u8; len];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| LdError::DecodeError("truncated: string payload incomplete".into()))?;
    String::from_utf8(buf).map_err(|_| LdError::DecodeError("invalid UTF-8 in string field".into()))
}

// ── CLI entry point ─────────────────────────────────────────────────────────

/// Run `axon ld` subcommand. Returns exit code.
pub fn run_ld(action: &str, file: &str) -> i32 {
    match action {
        "encode" => run_ld_encode(file),
        "decode" | "inspect" => run_ld_inspect(file),
        _ => {
            eprintln!("axon ld: unknown action '{action}'. Use: encode, decode, inspect");
            2
        }
    }
}

/// Encode an .axon file's ΛD declarations to .ld binary files.
fn run_ld_encode(file: &str) -> i32 {
    let source = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("X File not found: {file}");
            return 2;
        }
    };

    // Lex → Parse
    let tokens = match crate::lexer::Lexer::new(&source, file).tokenize() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("X Lexer error: {}", e.message);
            return 1;
        }
    };
    let mut parser = crate::parser::Parser::new(tokens);
    let program = match parser.parse() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("X Parse error: {}", e.message);
            return 1;
        }
    };

    // Extract ΛD declarations
    let mut count = 0;
    for decl in &program.declarations {
        if let crate::ast::Declaration::LambdaData(ld_def) = decl {
            let derivation = if ld_def.derivation.is_empty() {
                "raw"
            } else {
                &ld_def.derivation
            };
            let ld = match from_ir(
                &ld_def.name,
                &ld_def.ontology,
                ld_def.certainty,
                &ld_def.temporal_frame_start,
                &ld_def.temporal_frame_end,
                &ld_def.provenance,
                derivation,
            ) {
                Ok(ld) => ld,
                Err(e) => {
                    eprintln!("X {e}");
                    return 1;
                }
            };

            let bytes = match encode(&ld) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("X {e}");
                    return 1;
                }
            };

            let out_path = format!("{}.ld", ld_def.name);
            if let Err(e) = std::fs::write(&out_path, &bytes) {
                eprintln!("X Failed to write {out_path}: {e}");
                return 1;
            }
            println!(
                "  \u{2713} {} \u{2192} {out_path} ({} bytes, c={}, \u{03B4}={})",
                ld_def.name,
                bytes.len(),
                ld.certainty,
                ld.derivation.as_str()
            );
            count += 1;
        }
    }

    if count == 0 {
        eprintln!("X No lambda data declarations found in {file}");
        return 1;
    }
    println!("\n{count} \u{039B}D state vector(s) encoded.");
    0
}

/// Decode and inspect an .ld binary file.
fn run_ld_inspect(file: &str) -> i32 {
    let data = match std::fs::read(file) {
        Ok(d) => d,
        Err(_) => {
            eprintln!("X File not found: {file}");
            return 2;
        }
    };

    let ld = match decode(&data) {
        Ok(ld) => ld,
        Err(e) => {
            eprintln!("X {e}");
            return 1;
        }
    };

    println!("\u{03C8} = \u{27E8}T, V, E\u{27E9}  where  E = \u{27E8}c, \u{03C4}, \u{03C1}, \u{03B4}\u{27E9}\n");
    println!("  name:       {}", ld.name);
    println!("  T (ontology): {}", ld.ontology);
    println!("  V (payload):  {} bytes", ld.value.len());
    println!("  c (certainty): {}", ld.certainty);
    if !ld.temporal_frame_start.is_empty() {
        let tf = if ld.temporal_frame_end.is_empty() {
            ld.temporal_frame_start.clone()
        } else {
            format!("[{}, {}]", ld.temporal_frame_start, ld.temporal_frame_end)
        };
        println!("  \u{03C4} (temporal):  {tf}");
    }
    if !ld.provenance.is_empty() {
        println!("  \u{03C1} (provenance): {}", ld.provenance);
    }
    println!("  \u{03B4} (derivation): {}", ld.derivation.as_str());
    println!(
        "\n  format: \u{039B}D v{FORMAT_VERSION} ({} bytes)",
        data.len()
    );
    0
}

#[cfg(test)]
mod tests {
    use super::apply_provenance_ceiling;

    /// v2.5.0 — tainted-overriding: the ceiling is a CEILING.
    #[test]
    fn provenance_ceiling_is_a_ceiling_not_a_floor() {
        // input BELOW the ceiling → input wins (no laundering up).
        assert_eq!(apply_provenance_ceiling(0.40, 0.95), 0.40);
        // input ABOVE the ceiling → ceiling caps it.
        assert_eq!(apply_provenance_ceiling(0.99, 0.80), 0.80);
        // equal → either.
        assert_eq!(apply_provenance_ceiling(0.80, 0.80), 0.80);
    }

    /// Theorem 5.1 symmetry: it is `min`, commutative.
    #[test]
    fn provenance_ceiling_is_min() {
        assert_eq!(
            apply_provenance_ceiling(0.30, 0.70),
            apply_provenance_ceiling(0.70, 0.30)
        );
        assert_eq!(apply_provenance_ceiling(0.30, 0.70), 0.30);
    }

    /// Defensive clamping: out-of-range operands cannot escape [0,1].
    #[test]
    fn provenance_ceiling_clamps_out_of_range() {
        assert_eq!(apply_provenance_ceiling(1.5, 0.9), 0.9);
        assert_eq!(apply_provenance_ceiling(0.5, 2.0), 0.5);
        assert_eq!(apply_provenance_ceiling(-0.2, 0.9), 0.0);
    }
}
