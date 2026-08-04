//! §Fase 100.c/100.d — the OOXML reader: unzip an ingested DOCX/PPTX/XLSX into a
//! typed, bounded, born-`Untrusted` text tree — the *faithful* half of the
//! `parse ≠ infer` split (D100.1).
//!
//! **Born `Untrusted` (D100.2, reuses §98 verbatim).** A `.docx` from a
//! customer's inbox is exactly as adversarial as a scraped page: prompt
//! injection in comments / hidden runs / speaker notes, external relationship
//! targets (SSRF on open), DDE / field codes, embedded OLE, zip bombs, XML
//! entity expansion. The reader's output is born `EpistemicTaint::Untrusted`
//! and cannot reach an agent's beliefs without a shield (the §98.f barrier).
//!
//! **Parsed, never Inferred (D100.1/D100.14).** Everything this reader produces
//! is [`IngestProvenance::Parsed`] — a fact about the file, re-derivable from
//! the bytes, elevatable by a shield. It NEVER produces `Inferred` (OCR /
//! vision): that has no producer until §101, so the intermediate state is safe
//! by construction.
//!
//! **Bounded BEFORE parsed (D100.13).** Entry count, per-entry uncompressed
//! size, compression ratio (zip bomb), and total size are checked before the
//! XML is looked at. Entity expansion / external DTDs are refused outright. A
//! hostile document cannot OOM or hang the runtime.
//!
//! **Every threat is a typed refusal, never a silent fetch/expand (§6):**
//! external relationship targets, DDE fields, and OLE objects are
//! [`IngestError`]s — refused and (enterprise-side) audited, never followed.

use std::collections::BTreeMap;
use std::io::Read;

use sha2::{Digest, Sha256};

use crate::emcp::EpistemicTaint;

// ── Bounds (D100.13) ──────────────────────────────────────────────────────────

/// Max ZIP entries in an ingested package — a package with thousands of parts is
/// a decompression-amplification vector.
pub const MAX_ENTRIES: usize = 4096;
/// Max uncompressed bytes for a single entry (64 MiB).
pub const MAX_ENTRY_UNCOMPRESSED: u64 = 64 * 1024 * 1024;
/// Max total uncompressed bytes across the package (256 MiB).
pub const MAX_TOTAL_UNCOMPRESSED: u64 = 256 * 1024 * 1024;
/// Max compression ratio (uncompressed / compressed) before an entry is treated
/// as a zip bomb.
pub const MAX_COMPRESSION_RATIO: u64 = 200;

/// The hostile-input bounds. `Default` is the production posture.
#[derive(Debug, Clone)]
pub struct IngestBounds {
    pub max_entries: usize,
    pub max_entry_uncompressed: u64,
    pub max_total_uncompressed: u64,
    pub max_compression_ratio: u64,
}

impl Default for IngestBounds {
    fn default() -> Self {
        IngestBounds {
            max_entries: MAX_ENTRIES,
            max_entry_uncompressed: MAX_ENTRY_UNCOMPRESSED,
            max_total_uncompressed: MAX_TOTAL_UNCOMPRESSED,
            max_compression_ratio: MAX_COMPRESSION_RATIO,
        }
    }
}

// ── Provenance class (D100.1) — the keystone type ─────────────────────────────

// §Fase 118.b — `IngestProvenance` MOVED to `crate::ingest_provenance`, a
// leaf module. It is an epistemic concept, not an OOXML one, and living here
// made the §101 extraction contract depend on the OOXML reader (and `zip`)
// just to name it. Re-exported so existing call sites keep resolving.
pub use crate::ingest_provenance::IngestProvenance;

// ── Output ────────────────────────────────────────────────────────────────────

/// One extracted text run, in document order, tagged with its source part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRun {
    pub part: String,
    pub text: String,
}

/// An ingested document — the typed, born-`Untrusted`, Parsed text tree.
#[derive(Debug, Clone)]
pub struct IngestedDocument {
    /// `docx | pptx | xlsx`.
    pub format: String,
    /// Always `Untrusted` on read (D100.2).
    pub taint: EpistemicTaint,
    /// Always `Parsed` on read (D100.1) — never `Inferred` in §100.
    pub provenance: IngestProvenance,
    /// Extracted text runs, in document order.
    pub text: Vec<TextRun>,
    /// `sha256` per package part — the basis of the surgical-edit manifest
    /// (§100.e, D100.7).
    pub part_hashes: BTreeMap<String, String>,
    /// The raw package parts (name → bytes), preserved verbatim for the
    /// preserve-by-default surgical editor (D100.6). NEVER round-tripped through
    /// the §99 IR.
    pub parts: BTreeMap<String, Vec<u8>>,
}

impl IngestedDocument {
    /// The concatenated text (document order) — what a shield scans + what a
    /// `pix` navigator descends (D100.11).
    pub fn full_text(&self) -> String {
        self.text.iter().map(|r| r.text.as_str()).collect::<Vec<_>>().join("\n")
    }
}

/// Everything that can go wrong ingesting. Every variant is a typed refusal —
/// never a silent fetch, expand, or truncation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestError {
    NotAZip(String),
    UnknownFormat,
    TooManyEntries(usize),
    EntryTooLarge(String, u64),
    TotalTooLarge(u64),
    /// Compression ratio exceeded — a zip bomb.
    ZipBombRefused(String, u64),
    /// The XML declares a DTD / entity — refused (billion-laughs).
    EntityExpansionRefused(String),
    /// A relationship part names an external target — refused, never fetched.
    ExternalRelationshipRefused(String),
    /// A DDE field code — refused, never executed.
    DdeFieldRefused(String),
    /// An embedded OLE object — refused, never opened.
    OleObjectRefused(String),
    Malformed(String),
}

impl std::fmt::Display for IngestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use IngestError::*;
        match self {
            NotAZip(e) => write!(f, "not a valid OOXML package (zip): {e}"),
            UnknownFormat => write!(f, "unrecognised OOXML format (no word/xl/ppt part)"),
            TooManyEntries(n) => write!(f, "package has {n} entries (max {MAX_ENTRIES}) — refused"),
            EntryTooLarge(p, n) => write!(f, "part '{p}' is {n} bytes uncompressed — refused"),
            TotalTooLarge(n) => write!(f, "package is {n} bytes uncompressed — refused"),
            ZipBombRefused(p, r) => write!(f, "part '{p}' has compression ratio {r}× — zip bomb refused"),
            EntityExpansionRefused(p) => write!(f, "part '{p}' declares a DTD/entity — entity-expansion refused"),
            ExternalRelationshipRefused(t) => write!(f, "external relationship target '{t}' — refused, never fetched"),
            DdeFieldRefused(p) => write!(f, "DDE field in '{p}' — refused, never executed"),
            OleObjectRefused(p) => write!(f, "embedded OLE object '{p}' — refused, never opened"),
            Malformed(e) => write!(f, "malformed OOXML: {e}"),
        }
    }
}
impl std::error::Error for IngestError {}

// ── The reader ────────────────────────────────────────────────────────────────

/// §Fase 100.c — read an OOXML package from bytes into an
/// [`IngestedDocument`]. Bounds are enforced BEFORE parse; threats are typed
/// refusals; the output is born `Untrusted` + `Parsed`.
pub fn read_ooxml(bytes: &[u8], bounds: &IngestBounds) -> Result<IngestedDocument, IngestError> {
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).map_err(|e| IngestError::NotAZip(e.to_string()))?;

    if zip.len() > bounds.max_entries {
        return Err(IngestError::TooManyEntries(zip.len()));
    }

    let mut parts: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut part_hashes: BTreeMap<String, String> = BTreeMap::new();
    let mut total: u64 = 0;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| IngestError::Malformed(e.to_string()))?;
        let name = entry.name().to_string();
        // Skip directory entries.
        if name.ends_with('/') {
            continue;
        }
        let uncompressed = entry.size();
        let compressed = entry.compressed_size().max(1);
        // (bounds, BEFORE reading the bytes)
        if uncompressed > bounds.max_entry_uncompressed {
            return Err(IngestError::EntryTooLarge(name, uncompressed));
        }
        let ratio = uncompressed / compressed;
        if ratio > bounds.max_compression_ratio {
            return Err(IngestError::ZipBombRefused(name, ratio));
        }
        total = total.saturating_add(uncompressed);
        if total > bounds.max_total_uncompressed {
            return Err(IngestError::TotalTooLarge(total));
        }
        // (embedded OLE — refuse before reading)
        if name.contains("embeddings/") || name.to_ascii_lowercase().contains("oleobject") {
            return Err(IngestError::OleObjectRefused(name));
        }
        // Read the bytes (now bounded).
        let mut buf = Vec::with_capacity(uncompressed.min(bounds.max_entry_uncompressed) as usize);
        entry.read_to_end(&mut buf).map_err(|e| IngestError::Malformed(e.to_string()))?;
        part_hashes.insert(name.clone(), hex(&Sha256::digest(&buf)));
        parts.insert(name, buf);
    }

    // Determine the format.
    let format = if parts.keys().any(|k| k.starts_with("word/")) {
        "docx"
    } else if parts.keys().any(|k| k.starts_with("xl/")) {
        "xlsx"
    } else if parts.keys().any(|k| k.starts_with("ppt/")) {
        "pptx"
    } else {
        return Err(IngestError::UnknownFormat);
    };

    // Threat scans on the XML parts (entity expansion, external rels, DDE).
    for (name, bytes) in &parts {
        if !name.ends_with(".xml") && !name.ends_with(".rels") {
            continue;
        }
        let text = String::from_utf8_lossy(bytes);
        // (entity expansion / DTD — refuse)
        if name.ends_with(".xml") && (text.contains("<!DOCTYPE") || text.contains("<!ENTITY")) {
            return Err(IngestError::EntityExpansionRefused(name.clone()));
        }
        // (external relationship targets — refuse, never fetch)
        if name.ends_with(".rels") {
            if let Some(t) = external_relationship_target(&text) {
                return Err(IngestError::ExternalRelationshipRefused(t));
            }
        }
        // (DDE field codes — refuse)
        if name.ends_with(".xml") && has_dde(&text) {
            return Err(IngestError::DdeFieldRefused(name.clone()));
        }
    }

    // Extract text runs (bounded; regex over the known text parts — no XML
    // entity expansion possible).
    let mut text = Vec::new();
    for (name, bytes) in &parts {
        if is_text_part(name, format) {
            let body = String::from_utf8_lossy(bytes);
            for run in extract_text_runs(&body) {
                if !run.trim().is_empty() {
                    text.push(TextRun { part: name.clone(), text: run });
                }
            }
        }
    }

    Ok(IngestedDocument {
        format: format.to_string(),
        taint: EpistemicTaint::Untrusted,
        provenance: IngestProvenance::Parsed,
        text,
        part_hashes,
        parts,
    })
}

/// Is `name` a text-bearing part for `format`?
fn is_text_part(name: &str, format: &str) -> bool {
    match format {
        "docx" => name == "word/document.xml" || name.starts_with("word/footnotes") || name.starts_with("word/comments"),
        "pptx" => name.starts_with("ppt/slides/slide") && name.ends_with(".xml")
            || name.starts_with("ppt/notesSlides/"),
        "xlsx" => name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml")
            || name == "xl/sharedStrings.xml",
        _ => false,
    }
}

/// Extract `<w:t>` / `<a:t>` / `<t>` text-node contents (the faithful text runs).
fn extract_text_runs(xml: &str) -> Vec<String> {
    use regex::Regex;
    use std::sync::OnceLock;
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?s)<(?:w:t|a:t|t)(?:\s[^>]*)?>(.*?)</(?:w:t|a:t|t)>").expect("valid text regex")
    });
    re.captures_iter(xml)
        .filter_map(|c| c.get(1))
        .map(|m| xml_unescape(m.as_str()))
        .collect()
}

/// Minimal XML text unescaping (the inverse of the §99 writer's escaping).
fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Detect an external relationship target in a `.rels` part: `TargetMode=
/// "External"`, or an absolute `http(s)://` / `file://` target.
fn external_relationship_target(rels_xml: &str) -> Option<String> {
    if rels_xml.contains("TargetMode=\"External\"") {
        return Some("(TargetMode=External)".to_string());
    }
    use regex::Regex;
    use std::sync::OnceLock;
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#"Target="((?:https?|file|ftp)://[^"]+)""#).expect("valid rel regex"));
    re.captures(rels_xml).and_then(|c| c.get(1)).map(|m| m.as_str().to_string())
}

/// Detect a DDE field code (`DDEAUTO` / `DDE ` in a field instruction).
fn has_dde(xml: &str) -> bool {
    let up = xml.to_ascii_uppercase();
    up.contains("DDEAUTO") || up.contains("\"DDE\"") || up.contains(">DDE ") || up.contains("W:FLDSIMPLE W:INSTR=\"DDE")
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Build a minimal in-memory OOXML zip from (name, bytes) parts.
    fn build_zip(parts: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, data) in parts {
                zip.start_file(*name, opts).unwrap();
                zip.write_all(data).unwrap();
            }
            zip.finish().unwrap();
        }
        buf
    }

    fn docx_bytes(document_xml: &str) -> Vec<u8> {
        build_zip(&[
            ("[Content_Types].xml", b"<Types/>"),
            ("_rels/.rels", b"<Relationships/>"),
            ("word/document.xml", document_xml.as_bytes()),
        ])
    }

    #[test]
    fn reads_docx_text_born_untrusted_parsed() {
        let xml = r#"<w:document><w:body><w:p><w:r><w:t>Hello &amp; welcome</w:t></w:r></w:p></w:body></w:document>"#;
        let doc = read_ooxml(&docx_bytes(xml), &IngestBounds::default()).unwrap();
        assert_eq!(doc.format, "docx");
        assert_eq!(doc.taint, EpistemicTaint::Untrusted);
        assert_eq!(doc.provenance, IngestProvenance::Parsed);
        assert_eq!(doc.full_text(), "Hello & welcome");
        assert!(doc.part_hashes.contains_key("word/document.xml"));
    }

    #[test]
    fn never_constructs_inferred_in_this_fase() {
        // D100.14 — the reader only ever produces Parsed. (The vacuum test.)
        let doc = read_ooxml(&docx_bytes("<w:document/>"), &IngestBounds::default()).unwrap();
        assert_eq!(doc.provenance, IngestProvenance::Parsed);
        assert_ne!(doc.provenance, IngestProvenance::Inferred);
    }

    #[test]
    fn entity_expansion_is_refused() {
        let xml = "<!DOCTYPE lolz [<!ENTITY lol \"lol\">]><w:document/>";
        let err = read_ooxml(&docx_bytes(xml), &IngestBounds::default()).unwrap_err();
        assert!(matches!(err, IngestError::EntityExpansionRefused(_)));
    }

    #[test]
    fn external_relationship_is_refused_never_fetched() {
        let bytes = build_zip(&[
            ("word/document.xml", b"<w:document/>"),
            ("word/_rels/document.xml.rels", br#"<Relationships><Relationship Target="http://evil.example/x" TargetMode="External"/></Relationships>"#),
        ]);
        let err = read_ooxml(&bytes, &IngestBounds::default()).unwrap_err();
        assert!(matches!(err, IngestError::ExternalRelationshipRefused(_)));
    }

    #[test]
    fn dde_field_is_refused() {
        let xml = r#"<w:document><w:fldSimple w:instr="DDEAUTO c:\\evil"/></w:document>"#;
        let err = read_ooxml(&docx_bytes(xml), &IngestBounds::default()).unwrap_err();
        assert!(matches!(err, IngestError::DdeFieldRefused(_)));
    }

    #[test]
    fn ole_object_is_refused() {
        let bytes = build_zip(&[
            ("word/document.xml", b"<w:document/>"),
            ("word/embeddings/oleObject1.bin", b"\x00\x01\x02"),
        ]);
        let err = read_ooxml(&bytes, &IngestBounds::default()).unwrap_err();
        assert!(matches!(err, IngestError::OleObjectRefused(_)));
    }

    #[test]
    fn too_many_entries_refused() {
        let bounds = IngestBounds { max_entries: 2, ..Default::default() };
        let bytes = build_zip(&[("word/document.xml", b"<w:document/>"), ("a", b"x"), ("b", b"y")]);
        assert!(matches!(read_ooxml(&bytes, &bounds), Err(IngestError::TooManyEntries(_))));
    }

    #[test]
    fn provenance_ceilings_are_correct() {
        assert_eq!(IngestProvenance::Parsed.epistemic_ceiling(), "know");
        assert_eq!(IngestProvenance::Inferred.epistemic_ceiling(), "believe");
    }

    #[test]
    fn not_a_zip_is_typed() {
        assert!(matches!(read_ooxml(b"not a zip", &IngestBounds::default()), Err(IngestError::NotAZip(_))));
    }
}
