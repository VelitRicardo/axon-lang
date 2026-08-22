//! AXON Audit Evidence Engine — EvidencePackager
//!
//! Direct port of `axon/runtime/esk/audit_engine/evidence_packager.py`.
//!
//! Bundles every artifact an external auditor typically requests into a
//! single ZIP file, ready for hand-off. The package is deterministic
//! (byte-identical on equal inputs) and carries a manifest that
//! enumerates every file with its SHA-256 digest so the auditor can
//! verify nothing was altered during transport.
//!
//! Typical contents:
//!   - `MANIFEST.json`          — package index with per-file SHA-256
//!   - `README.md`              — intake note for the auditor
//!   - `program_sbom.json`      — SupplyChainSBOM
//!   - `program_dossier.json`   — ComplianceDossier
//!   - `in_toto_statement.json` — SLSA Provenance v1 attestation
//!   - `provenance_chain.json`  — Merkle-linked runtime events (if any)
//!   - `risk_register.json`     — ISO 27005-shaped risk register
//!   - `gap_analysis/`          — one file per framework
//!   - `control_statements/`    — one file per framework
//!   - `source/`                — snapshot of the `.axon` source files

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::ir_nodes::IRProgram;

use super::super::attestation::{
    generate_dossier, generate_in_toto_statement, generate_sbom,
};
use super::control_statements::{generate_control_statements, statements_to_value};
use super::frameworks::all_frameworks;
use super::gap_analyzer::analyze_all;
use super::risk_register::{generate_risk_register, risk_register_to_value};

// The reference Python passes these as keyword-argument defaults into
// `generate_in_toto_statement`; the Rust port requires them explicitly.
const IN_TOTO_BUILDER_ID: &str = "https://axon-lang.io/builders/compiler@v1";
const IN_TOTO_SUBJECT_NAME: &str = "axon-program";

// ═══════════════════════════════════════════════════════════════════
//  In-memory bundle
// ═══════════════════════════════════════════════════════════════════

/// In-memory representation of a packaged audit bundle.
///
/// `files` uses a `BTreeMap` so iteration is naturally sorted — critical
/// for byte-identical ZIP output on equal inputs.
#[derive(Debug, Clone)]
pub struct EvidencePackage {
    pub program_hash: String,
    pub files: BTreeMap<String, Vec<u8>>,
}

impl EvidencePackage {
    pub fn new(program_hash: impl Into<String>) -> Self {
        EvidencePackage {
            program_hash: program_hash.into(),
            files: BTreeMap::new(),
        }
    }

    pub fn filenames(&self) -> Vec<String> {
        // BTreeMap already sorted; clone keys.
        self.files.keys().cloned().collect()
    }

    pub fn sha256_of(&self, filename: &str) -> Option<String> {
        self.files.get(filename).map(|bytes| {
            let mut hasher = Sha256::new();
            hasher.update(bytes);
            let digest = hasher.finalize();
            let mut s = String::with_capacity(64);
            for b in digest {
                s.push_str(&format!("{:02x}", b));
            }
            s
        })
    }

    /// Serialize to a single ZIP byte blob with deterministic ordering.
    pub fn to_zip_bytes(&self) -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();
        {
            let cursor = Cursor::new(&mut buf);
            let mut zip = zip::ZipWriter::new(cursor);
            // Fixed modification date so the archive is reproducible.
            let fixed_time = zip::DateTime::from_date_and_time(2026, 1, 1, 0, 0, 0)
                .expect("valid DateTime constants");
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .last_modified_time(fixed_time);
            // BTreeMap yields keys in sorted order — this matches Python's
            // `sorted(self.files.keys())` traversal.
            for (name, bytes) in &self.files {
                zip.start_file(name.as_str(), options)
                    .expect("zip start_file");
                zip.write_all(bytes).expect("zip write_all");
            }
            zip.finish().expect("zip finish");
        }
        buf
    }

    pub fn write_zip<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        let out = path.as_ref().to_path_buf();
        std::fs::write(&out, self.to_zip_bytes()).expect("write evidence zip");
        out
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Canonical JSON helper
// ═══════════════════════════════════════════════════════════════════

/// Canonical JSON encoding: sorted keys, 2-space indent,
/// `ensure_ascii=True` (Python `json.dumps(payload, sort_keys=True,
/// indent=2)` — non-ASCII is escaped as `\uXXXX` so the bytes match the
/// Python CLI output.
fn j(payload: &Value) -> Vec<u8> {
    let sorted = sort_value(payload);
    let raw = serde_json::to_string_pretty(&sorted).expect("serialise canonical JSON");
    escape_non_ascii(&raw).into_bytes()
}

/// Mirror `audit_cli::escape_non_ascii` — lives here to avoid a cross-
/// module dependency.
fn escape_non_ascii(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if (c as u32) < 0x80 {
            out.push(c);
        } else {
            let code = c as u32;
            if code <= 0xFFFF {
                out.push_str(&format!("\\u{:04x}", code));
            } else {
                let v = code - 0x10000;
                let hi = 0xD800 + (v >> 10);
                let lo = 0xDC00 + (v & 0x3FF);
                out.push_str(&format!("\\u{:04x}\\u{:04x}", hi, lo));
            }
        }
    }
    out
}

fn sort_value(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let mut out = Map::new();
            for k in keys {
                out.insert(k.clone(), sort_value(&m[k]));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(sort_value).collect()),
        _ => v.clone(),
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Builder
// ═══════════════════════════════════════════════════════════════════

/// Assemble the audit-ready evidence package for `program`.
///
/// The Python reference accepts an in-memory `ProvenanceChain` plus
/// payloads; since the Rust `ProvenanceChain<S>` is generic over a
/// `Signer` trait, this port takes the chain entries / payloads as
/// already-materialised `Value` lists. Pass `None` to both to skip
/// emission of `provenance_chain.json`.
pub fn build_evidence_package(
    program: &IRProgram,
    axon_version: &str,
    provenance_entries: Option<Vec<Value>>,
    provenance_payloads: Option<Vec<Value>>,
    source_files: Option<BTreeMap<String, String>>,
    auditor_note: &str,
) -> EvidencePackage {
    let sbom = generate_sbom(program, axon_version);
    let dossier = generate_dossier(program, axon_version);
    let in_toto = generate_in_toto_statement(
        program,
        axon_version,
        IN_TOTO_BUILDER_ID,
        IN_TOTO_SUBJECT_NAME,
    );
    let risks = generate_risk_register(program);
    let gap = analyze_all(program);

    let mut pkg = EvidencePackage::new(sbom.program_hash.clone());

    pkg.files.insert("program_sbom.json".into(), j(&sbom.to_value()));
    pkg.files
        .insert("program_dossier.json".into(), j(&dossier.to_value()));
    pkg.files
        .insert("in_toto_statement.json".into(), j(&in_toto.to_value()));
    pkg.files.insert(
        "risk_register.json".into(),
        j(&risk_register_to_value(&risks)),
    );

    // Gap analysis per framework — iterate the BTreeMap (keys already
    // sorted alphabetically).
    for (framework_name, analysis) in &gap {
        pkg.files.insert(
            format!("gap_analysis/{}.json", framework_name),
            j(&analysis.to_value()),
        );
    }

    // Control statements per framework — follow the canonical
    // `all_frameworks()` order.
    for f in all_frameworks() {
        let statements = generate_control_statements(program, f);
        pkg.files.insert(
            format!("control_statements/{}.json", f.as_str()),
            j(&statements_to_value(&statements, f)),
        );
    }

    // v4.6.0 — what NO gate produced, in its own file.
    //
    // Every other artifact in this package is derived: the SBOM, the dossier,
    // the control statements, the gap analysis all come from something the
    // compiler decided. Attestations are the opposite — they are the part a
    // person asserted because the compiler could not decide it, and mixing them
    // into a derived file would erase exactly the distinction that makes the
    // package worth reading.
    //
    // So they get a filename of their own, and it is absent entirely when the
    // program has none: a package with no `attestations.json` says "nothing was
    // signed here", which is a true and useful thing for an auditor to see.
    if !program.attestations.is_empty() {
        let entries: Vec<Value> = program
            .attestations
            .iter()
            .map(|a| {
                let mut m = Map::new();
                m.insert("name".into(), a.name.clone().into());
                m.insert("for".into(), a.for_type.clone().into());
                m.insert("basis".into(), a.basis.clone().into());
                m.insert("citation".into(), a.citation.clone().into());
                m.insert(
                    "residual".into(),
                    Value::Array(a.residual.iter().cloned().map(Value::from).collect()),
                );
                m.insert("by".into(), a.by.clone().into());
                m.insert("on".into(), a.on.clone().into());
                Value::Object(m)
            })
            .collect();
        let mut blob = Map::new();
        blob.insert("schema".into(), "axon.esk.attestations.v1".into());
        // Said in the file itself, not only in the docs: a reader who opens
        // this one artifact must not mistake it for compiler output.
        blob.insert(
            "asserted_by_humans".into(),
            "Every entry here is a determination a person signed. The compiler \
             verified only that each is complete and cites a method the regulation \
             defines — never that it is true."
                .into(),
        );
        blob.insert("count".into(), (entries.len() as i64).into());
        blob.insert("entries".into(), Value::Array(entries));
        pkg.files
            .insert("attestations.json".into(), j(&Value::Object(blob)));
    }

    // Provenance chain + payloads.
    if let Some(entries) = provenance_entries {
        let mut chain_blob = Map::new();
        chain_blob.insert("schema".into(), "axon.esk.provenance_chain.v1".into());
        chain_blob.insert("genesis".into(), "0".repeat(64).into());
        chain_blob.insert("count".into(), (entries.len() as i64).into());
        chain_blob.insert("entries".into(), Value::Array(entries));
        if let Some(payloads) = provenance_payloads {
            chain_blob.insert("payloads".into(), Value::Array(payloads));
        }
        pkg.files
            .insert("provenance_chain.json".into(), j(&Value::Object(chain_blob)));
    }

    // Source snapshot.
    if let Some(files) = source_files {
        for (fname, source_text) in files {
            let safe_name = fname.replace('\\', "/").trim_start_matches('/').to_string();
            pkg.files
                .insert(format!("source/{}", safe_name), source_text.into_bytes());
        }
    }

    // README — last, so it can reference the SHA-256s of the rest.
    let readme = build_readme(&sbom.program_hash, auditor_note);
    pkg.files.insert("README.md".into(), readme.into_bytes());

    // MANIFEST — absolutely last, after every other file. Note we iterate
    // `pkg.files` in sorted order (BTreeMap) matching Python's
    // `sorted(pkg.files.keys())`.
    // Python semantics: file_count is computed BEFORE MANIFEST.json is
    // added to the package, so it records the count of non-manifest
    // artefacts. Do not +1 here.
    let file_count = pkg.files.len();
    let file_entries: Vec<Value> = pkg
        .files
        .keys()
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .map(|name| {
            let mut m = Map::new();
            let sha = pkg.sha256_of(&name).unwrap_or_default();
            let size = pkg.files.get(&name).map(|b| b.len()).unwrap_or(0);
            m.insert("name".into(), name.into());
            m.insert("sha256".into(), sha.into());
            m.insert("size".into(), (size as i64).into());
            Value::Object(m)
        })
        .collect();
    let mut manifest = Map::new();
    manifest.insert("schema".into(), "axon.esk.evidence_manifest.v1".into());
    manifest.insert("axon_version".into(), axon_version.into());
    manifest.insert("program_hash".into(), sbom.program_hash.clone().into());
    manifest.insert("file_count".into(), (file_count as i64).into());
    manifest.insert("files".into(), Value::Array(file_entries));
    pkg.files
        .insert("MANIFEST.json".into(), j(&Value::Object(manifest)));

    pkg
}

fn build_readme(program_hash: &str, auditor_note: &str) -> String {
    let mut lines: Vec<String> = vec![
        "# AXON Audit Evidence Package".into(),
        String::new(),
        format!("**Program hash:** `{}`", program_hash),
        String::new(),
        "## What is in this package".into(),
        String::new(),
        "This ZIP bundles the deterministic audit artifacts produced by the".into(),
        "Axon compiler + ESK runtime.  Every JSON file is canonical-encoded".into(),
        "and carries its own SHA-256 in `MANIFEST.json`.".into(),
        String::new(),
        "| File | Purpose |".into(),
        "|---|---|".into(),
        "| `program_sbom.json`              | Software Bill of Materials (deterministic) |".into(),
        "| `program_dossier.json`           | Regulatory compliance dossier |".into(),
        "| `in_toto_statement.json`         | SLSA Provenance v1 attestation |".into(),
        "| `risk_register.json`             | ISO 27005-shaped risk register |".into(),
        "| `gap_analysis/*.json`            | Per-framework gap analysis |".into(),
        "| `control_statements/*.json`      | Pre-populated implementation statements |".into(),
        // v4.6.0 — the one file in here nothing computed. The wording is the
        // point: an auditor scanning this table must be able to tell the
        // derived artifacts from the signed one without opening either.
        "| `attestations.json`              | Human determinations — SIGNED, not derived (if any) |".into(),
        "| `provenance_chain.json`          | Runtime Merkle chain (if provided) |".into(),
        "| `source/*.axon`                  | Source snapshot at package time |".into(),
        String::new(),
        "## Verifying the package".into(),
        String::new(),
        "Every entry in `MANIFEST.json` carries a SHA-256 hash of the file".into(),
        "contents.  An auditor re-running `sha256sum` on each file MUST see".into(),
        "the same digest — the package is deterministic.  The program_hash".into(),
        "inside `MANIFEST.json` equals the `program_hash` inside".into(),
        "`program_sbom.json` — any divergence signals tampering.".into(),
        String::new(),
        "## Frameworks covered".into(),
        String::new(),
        "- SOC 2 Type II (AICPA Trust Services Criteria)".into(),
        "- ISO/IEC 27001:2022 (Annex A subset)".into(),
        "- FIPS 140-3 (scaffold + readiness)".into(),
        "- Common Criteria EAL 4+ (SFR / SAR readiness)".into(),
        String::new(),
    ];
    if !auditor_note.is_empty() {
        lines.extend([
            "## Auditor note".into(),
            String::new(),
            auditor_note.to_string(),
            String::new(),
        ]);
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::super::frameworks::all_frameworks;
    use super::*;
    use crate::ir_generator::IRGenerator;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use std::io::Read;

    fn compile(source: &str) -> IRProgram {
        let tokens = Lexer::new(source, "t").tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        IRGenerator::new().generate(&program)
    }

    fn full_program() -> IRProgram {
        compile(r#"
            type R compliance [HIPAA] { x: String }
            flow F(r: R) -> R { step S { ask: "x" output: R } }
            shield G {
                scan: [prompt_injection]
                on_breach: halt
                severity: high
                compliance: [HIPAA]
            }
            axonendpoint E {
                method: POST path: "/p" body: R execute: F output: R
                shield: G
                compliance: [HIPAA]
            }
            resource Db { kind: postgres lifetime: linear }
            fabric Vpc { provider: aws }
            manifest M { resources: [Db] fabric: Vpc compliance: [HIPAA] }
            observe O from M { sources: [prom] quorum: 1 }
            reconcile Rec { observe: O }
            lease L { resource: Db duration: 30m }
            immune I { watch: [O] scope: tenant }
            reflex Rf { trigger: I on_level: doubt action: quarantine scope: tenant }
            heal H { source: I scope: tenant }
        "#)
    }

    #[test]
    fn package_contains_expected_top_level_files() {
        let ir = full_program();
        let mut sources: BTreeMap<String, String> = BTreeMap::new();
        sources.insert("prog.axon".into(), "// src".into());
        let pkg =
            build_evidence_package(&ir, "1.0.0", None, None, Some(sources), "");
        let names: std::collections::HashSet<String> =
            pkg.filenames().into_iter().collect();
        for required in [
            "MANIFEST.json",
            "README.md",
            "program_sbom.json",
            "program_dossier.json",
            "in_toto_statement.json",
            "risk_register.json",
        ] {
            assert!(names.contains(required), "missing {}", required);
        }
    }

    #[test]
    fn per_framework_files_exist() {
        let ir = full_program();
        let pkg = build_evidence_package(&ir, "1.0.0", None, None, None, "");
        let names: std::collections::HashSet<String> =
            pkg.filenames().into_iter().collect();
        for f in all_frameworks() {
            assert!(
                names.contains(&format!("gap_analysis/{}.json", f.as_str())),
                "missing gap_analysis for {:?}",
                f
            );
            assert!(
                names.contains(&format!("control_statements/{}.json", f.as_str())),
                "missing control_statements for {:?}",
                f
            );
        }
    }

    #[test]
    fn manifest_sha256_matches_content_for_every_file() {
        let ir = full_program();
        let pkg = build_evidence_package(&ir, "1.0.0", None, None, None, "");
        let manifest_bytes = pkg
            .files
            .get("MANIFEST.json")
            .expect("MANIFEST present");
        let manifest: Value = serde_json::from_slice(manifest_bytes).unwrap();
        let files = manifest["files"].as_array().unwrap();
        for entry in files {
            let name = entry["name"].as_str().unwrap();
            if name == "MANIFEST.json" {
                // MANIFEST does not list itself (Python skips too).
                continue;
            }
            let expected = entry["sha256"].as_str().unwrap();
            let actual = pkg.sha256_of(name).unwrap();
            assert_eq!(actual, expected, "sha mismatch for {}", name);
        }
    }

    #[test]
    fn zip_opens_and_contains_all_files() {
        let ir = full_program();
        let pkg = build_evidence_package(&ir, "1.0.0", None, None, None, "");
        let bytes = pkg.to_zip_bytes();
        assert!(!bytes.is_empty(), "zip should not be empty");
        let mut archive =
            zip::ZipArchive::new(Cursor::new(bytes)).expect("zip parses");
        let mut in_zip: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for i in 0..archive.len() {
            let f = archive.by_index(i).unwrap();
            in_zip.insert(f.name().to_string());
        }
        let in_pkg: std::collections::HashSet<String> =
            pkg.filenames().into_iter().collect();
        assert_eq!(in_zip, in_pkg);
    }

    #[test]
    fn zip_is_byte_identical_on_equal_input() {
        let ir = full_program();
        let a =
            build_evidence_package(&ir, "1.0.0", None, None, None, "").to_zip_bytes();
        let b =
            build_evidence_package(&ir, "1.0.0", None, None, None, "").to_zip_bytes();
        assert_eq!(a, b, "evidence ZIP must be deterministic");
    }

    #[test]
    fn source_snapshot_and_auditor_note_surface() {
        let ir = full_program();
        let mut sources: BTreeMap<String, String> = BTreeMap::new();
        sources.insert("prog.axon".into(), "// content\n".into());
        let pkg = build_evidence_package(
            &ir,
            "1.0.0",
            None,
            None,
            Some(sources),
            "Engaged with FirmX on 2026-04",
        );
        let src = pkg
            .files
            .get("source/prog.axon")
            .expect("source file present");
        assert!(std::str::from_utf8(src).unwrap().contains("// content"));
        let readme = pkg.files.get("README.md").expect("README present");
        let readme_text = std::str::from_utf8(readme).unwrap();
        assert!(readme_text.contains("Engaged with FirmX"));
    }

    #[test]
    fn provenance_chain_emitted_when_entries_supplied() {
        let ir = full_program();
        let entries = vec![serde_json::json!({"seq": 0, "data_hash": "abc"})];
        let payloads = vec![serde_json::json!({"event": "start"})];
        let pkg = build_evidence_package(
            &ir,
            "1.0.0",
            Some(entries),
            Some(payloads),
            None,
            "",
        );
        let chain = pkg
            .files
            .get("provenance_chain.json")
            .expect("chain emitted");
        let parsed: Value = serde_json::from_slice(chain).unwrap();
        assert_eq!(parsed["schema"], "axon.esk.provenance_chain.v1");
        assert_eq!(parsed["count"], 1);
        assert_eq!(
            parsed["genesis"].as_str().unwrap().len(),
            64,
            "genesis hash should be 64 zeros"
        );
        assert!(parsed["payloads"].is_array());
    }

    #[test]
    fn zip_entries_round_trip_through_archive() {
        let ir = full_program();
        let pkg = build_evidence_package(&ir, "1.0.0", None, None, None, "");
        let bytes = pkg.to_zip_bytes();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        // Read README and confirm content equals in-memory bytes.
        let mut buf = Vec::new();
        archive
            .by_name("README.md")
            .unwrap()
            .read_to_end(&mut buf)
            .unwrap();
        assert_eq!(
            buf,
            *pkg.files.get("README.md").unwrap(),
            "README content must round-trip"
        );
    }

    // ── v4.6.0 — the signed half travels, and is not mistaken for the rest ──

    fn attested_program() -> IRProgram {
        compile(
            r#"
            type R compliance [HIPAA] { x: String }
            type Clean { note: String }
            attest Determination {
                for: Clean
                basis: safe_harbor
                residual: [other_unique_identifier, actual_knowledge]
                by: "Compliance Officer, Acme Health"
                on: "2026-08-21"
            }
            flow F(r: R) -> R { step S { ask: "x" output: R } }
        "#,
        )
    }

    #[test]
    fn an_attestation_reaches_the_evidence_package() {
        // The whole point of `attest` is that the determination OUTLIVES the run
        // that needed it. A field the compiler stores and the package drops is a
        // signature nobody can find.
        let pkg = build_evidence_package(&attested_program(), "4.6.0", None, None, None, "");
        let blob = pkg
            .files
            .get("attestations.json")
            .expect("a program that signs a determination must ship it");
        let v: Value = serde_json::from_slice(blob).expect("canonical JSON");

        assert_eq!(v["count"], 1);
        let e = &v["entries"][0];
        assert_eq!(e["for"], "Clean");
        assert_eq!(e["basis"], "safe_harbor");
        // The citation is resolved at lowering so the reader does not have to
        // know the regulation to print it.
        assert_eq!(e["citation"], "45 CFR 164.514(b)(2)");
        assert_eq!(e["by"], "Compliance Officer, Acme Health");
        assert_eq!(e["on"], "2026-08-21");
        assert_eq!(e["residual"][1], "actual_knowledge");
    }

    #[test]
    fn the_package_says_this_file_was_signed_and_not_computed() {
        // EVERY OTHER FILE IN THE BUNDLE IS DERIVED. If an auditor reads this one
        // as compiler output, the separation between what was proved and what was
        // asserted — the entire reason the file exists — is gone.
        let pkg = build_evidence_package(&attested_program(), "4.6.0", None, None, None, "");
        let blob = pkg.files.get("attestations.json").unwrap();
        let v: Value = serde_json::from_slice(blob).unwrap();
        let disclaimer = v["asserted_by_humans"].as_str().unwrap_or_default();
        assert!(
            disclaimer.contains("person signed") && disclaimer.contains("never that it is true"),
            "the file must disclaim, in itself, what the compiler did NOT check: {disclaimer}"
        );

        let readme = String::from_utf8(pkg.files.get("README.md").unwrap().clone()).unwrap();
        assert!(
            readme.contains("attestations.json") && readme.contains("SIGNED, not derived"),
            "the index an auditor scans first must make the distinction visible"
        );
    }

    #[test]
    fn a_program_that_signs_nothing_ships_no_attestation_file() {
        // Absence is informative here: no `attestations.json` means nothing was
        // signed, which is a true and useful thing for an auditor to see. An
        // empty file with `count: 0` would read as a form someone left blank.
        let pkg = build_evidence_package(&full_program(), "4.6.0", None, None, None, "");
        assert!(!pkg.files.contains_key("attestations.json"));
    }
}
