//! v2.54.0 — the surgical edit engine: mutate an ingested OOXML package in
//! place, touch only the targeted parts, leave every other part BYTE-IDENTICAL,
//! and emit a machine-checkable **per-part hash manifest** proving it (the design decision /
//! the design decision).
//!
//! **Preserve-by-default.** The editor NEVER round-trips through v2.53.0's
//! authoring IR — that IR models a bounded subset, so parsing a foreign
//! `.docx` into it and re-serialising would silently destroy tracked changes,
//! comments, SmartArt, custom parts, and unknown namespaces. Instead it keeps
//! the original package bytes and mutates only the targeted XML, re-zipping
//! deterministically (the v2.53.0 writer discipline).
//!
//! **The manifest is the payoff.** For every part it records the
//! `sha256` before and after and whether it was touched — so an auditor can
//! verify an edit changed `word/document.xml` and *nothing else*: no macro
//! added, no relationship rewritten, no custom part dropped. No document library
//! on any runtime offers this.
//!
//! **Edits inherit taint.** Opening an `Untrusted` document, editing a
//! cell, and saving does NOT produce a trusted document — laundering by round-
//! trip is closed. The output carries the input's taint verbatim.

use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use sha2::{Digest, Sha256};

use crate::emcp::EpistemicTaint;
use crate::ooxml_read::IngestedDocument;

/// A surgical edit targeting a single package part. Both variants are exact +
/// bounded; neither reinterprets the document model.
#[derive(Debug, Clone)]
pub enum PartEdit {
    /// Replace an entire part's bytes (e.g. a fully re-rendered `document.xml`).
    Replace { part: String, new_bytes: Vec<u8> },
    /// Replace the first occurrence of `find` with `replace` inside a part's
    /// UTF-8 text — the surgical, minimal-blast-radius edit.
    ReplaceText { part: String, find: String, replace: String },
}

impl PartEdit {
    fn target(&self) -> &str {
        match self {
            PartEdit::Replace { part, .. } | PartEdit::ReplaceText { part, .. } => part,
        }
    }
}

/// Per-part before/after hashes — the blast-radius proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartManifestEntry {
    pub part: String,
    pub before_sha256: String,
    pub after_sha256: String,
    pub touched: bool,
}

/// The result of a surgical edit: the new bytes + the manifest + the inherited
/// taint.
#[derive(Debug, Clone)]
pub struct EditedDocument {
    pub bytes: Vec<u8>,
    pub sha256_hex: String,
    /// Inherited from the input — never elevated by the edit.
    pub taint: EpistemicTaint,
    pub manifest: Vec<PartManifestEntry>,
}

impl EditedDocument {
    /// The parts this edit touched (the proven blast radius).
    pub fn touched_parts(&self) -> Vec<&str> {
        self.manifest.iter().filter(|m| m.touched).map(|m| m.part.as_str()).collect()
    }
    /// Whether the blast radius is EXACTLY `expected` (the manifest assertion).
    pub fn touched_exactly(&self, expected: &[&str]) -> bool {
        let mut got = self.touched_parts();
        got.sort_unstable();
        let mut want: Vec<&str> = expected.to_vec();
        want.sort_unstable();
        got == want
    }
}

/// Why an edit was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The edit targets a part not present in the source package (section 6).
    TargetPartMissing(String),
    /// `ReplaceText` found no occurrence of `find` in the target part.
    TextNotFound(String, String),
    /// Re-serialisation failed.
    Encode(String),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::TargetPartMissing(p) => write!(f, "edit target part '{p}' does not exist in the source package"),
            EditError::TextNotFound(p, s) => write!(f, "text '{s}' not found in part '{p}'"),
            EditError::Encode(e) => write!(f, "re-serialisation failed: {e}"),
        }
    }
}
impl std::error::Error for EditError {}

/// v2.54.0 — apply surgical `edits` to an ingested document, preserve-by-
/// default, and return the new bytes + the per-part hash manifest. The taint is
/// inherited. Every untouched part is byte-identical.
pub fn edit_document(doc: &IngestedDocument, edits: &[PartEdit]) -> Result<EditedDocument, EditError> {
    // (1) validate every target exists — before mutating anything.
    for e in edits {
        if !doc.parts.contains_key(e.target()) {
            return Err(EditError::TargetPartMissing(e.target().to_string()));
        }
    }

    // (2) apply edits to a clone of the parts (preserve-by-default).
    let mut new_parts = doc.parts.clone();
    for e in edits {
        match e {
            PartEdit::Replace { part, new_bytes } => {
                new_parts.insert(part.clone(), new_bytes.clone());
            }
            PartEdit::ReplaceText { part, find, replace } => {
                let current = new_parts.get(part).expect("validated above");
                let text = String::from_utf8_lossy(current);
                if !text.contains(find.as_str()) {
                    return Err(EditError::TextNotFound(part.clone(), find.clone()));
                }
                // Replace the FIRST occurrence only (surgical).
                let edited = text.replacen(find.as_str(), replace, 1);
                new_parts.insert(part.clone(), edited.into_bytes());
            }
        }
    }

    // (3) build the manifest (before/after per part).
    let mut manifest = Vec::with_capacity(new_parts.len());
    for (name, new_bytes) in &new_parts {
        let before = doc
            .part_hashes
            .get(name)
            .cloned()
            .unwrap_or_default();
        let after = hex(&Sha256::digest(new_bytes));
        let touched = before != after;
        manifest.push(PartManifestEntry {
            part: name.clone(),
            before_sha256: before,
            after_sha256: after,
            touched,
        });
    }

    // (4) re-zip deterministically (the v2.53.0 writer discipline: fixed DateTime,
    // Deflated, BTreeMap order).
    let bytes = zip_parts(&new_parts).map_err(EditError::Encode)?;
    let sha256_hex = hex(&Sha256::digest(&bytes));

    Ok(EditedDocument {
        bytes,
        sha256_hex,
        // the edit inherits the input's taint — never elevated.
        taint: doc.taint,
        manifest,
    })
}

fn zip_parts(parts: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, String> {
    let mut buf: Vec<u8> = Vec::new();
    {
        let cursor = Cursor::new(&mut buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let fixed = zip::DateTime::from_date_and_time(2026, 1, 1, 0, 0, 0)
            .map_err(|_| "bad DateTime".to_string())?;
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(fixed);
        for (name, data) in parts {
            zip.start_file(name.as_str(), opts).map_err(|e| e.to_string())?;
            zip.write_all(data).map_err(|e| e.to_string())?;
        }
        zip.finish().map_err(|e| e.to_string())?;
    }
    Ok(buf)
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
    use crate::ooxml_read::{read_ooxml, IngestBounds};

    fn build_zip(parts: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let cursor = Cursor::new(&mut buf);
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

    /// A docx fixture carrying a comment, a custom part, and an unknown
    /// namespace — the parts the manifest must prove untouched (D100 section 8).
    fn rich_docx() -> IngestedDocument {
        let bytes = build_zip(&[
            ("[Content_Types].xml", b"<Types/>"),
            ("_rels/.rels", b"<Relationships/>"),
            ("word/document.xml", br#"<w:document><w:body><w:p><w:r><w:t>Total: 100</w:t></w:r></w:p></w:body></w:document>"#),
            ("word/comments.xml", b"<w:comments><w:comment>reviewer note</w:comment></w:comments>"),
            ("customXml/item1.xml", b"<myns:custom xmlns:myns=\"urn:acme\">keep me</myns:custom>"),
        ]);
        read_ooxml(&bytes, &IngestBounds::default()).unwrap()
    }

    #[test]
    fn surgical_edit_touches_only_the_target_and_manifest_proves_it() {
        let doc = rich_docx();
        let out = edit_document(
            &doc,
            &[PartEdit::ReplaceText {
                part: "word/document.xml".into(),
                find: "Total: 100".into(),
                replace: "Total: 250".into(),
            }],
        )
        .unwrap();
        // The manifest proves the blast radius is EXACTLY document.xml.
        assert!(out.touched_exactly(&["word/document.xml"]), "touched: {:?}", out.touched_parts());
        // Every other part is byte-identical (before == after in the manifest).
        for m in &out.manifest {
            if m.part != "word/document.xml" {
                assert!(!m.touched, "part {} must be untouched", m.part);
                assert_eq!(m.before_sha256, m.after_sha256);
            }
        }
    }

    #[test]
    fn untouched_parts_are_byte_identical_in_the_output() {
        let doc = rich_docx();
        let comments_before = doc.parts.get("word/comments.xml").unwrap().clone();
        let custom_before = doc.parts.get("customXml/item1.xml").unwrap().clone();
        let out = edit_document(
            &doc,
            &[PartEdit::ReplaceText {
                part: "word/document.xml".into(),
                find: "100".into(),
                replace: "250".into(),
            }],
        )
        .unwrap();
        // Re-read the edited package and confirm the untouched parts survived.
        let reread = read_ooxml(&out.bytes, &IngestBounds::default()).unwrap();
        assert_eq!(reread.parts.get("word/comments.xml").unwrap(), &comments_before);
        assert_eq!(reread.parts.get("customXml/item1.xml").unwrap(), &custom_before);
    }

    #[test]
    fn edit_inherits_taint_no_laundering() {
        let doc = rich_docx();
        assert_eq!(doc.taint, EpistemicTaint::Untrusted);
        let out = edit_document(&doc, &[PartEdit::ReplaceText {
            part: "word/document.xml".into(),
            find: "100".into(),
            replace: "1".into(),
        }]).unwrap();
        // Opening an Untrusted doc + editing + saving stays Untrusted.
        assert_eq!(out.taint, EpistemicTaint::Untrusted);
    }

    #[test]
    fn edit_is_deterministic() {
        let doc = rich_docx();
        let e = || edit_document(&doc, &[PartEdit::ReplaceText {
            part: "word/document.xml".into(), find: "100".into(), replace: "9".into(),
        }]).unwrap();
        assert_eq!(e().sha256_hex, e().sha256_hex);
    }

    #[test]
    fn missing_target_part_is_refused() {
        let doc = rich_docx();
        let err = edit_document(&doc, &[PartEdit::Replace {
            part: "word/ghost.xml".into(), new_bytes: b"x".to_vec(),
        }]).unwrap_err();
        assert!(matches!(err, EditError::TargetPartMissing(_)));
    }

    #[test]
    fn text_not_found_is_refused() {
        let doc = rich_docx();
        let err = edit_document(&doc, &[PartEdit::ReplaceText {
            part: "word/document.xml".into(), find: "nonexistent".into(), replace: "x".into(),
        }]).unwrap_err();
        assert!(matches!(err, EditError::TextNotFound(_, _)));
    }
}
