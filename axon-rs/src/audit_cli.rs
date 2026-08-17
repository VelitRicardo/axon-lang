//! AXON CLI — ESK audit commands (§Fase 8.6 CLI parity).
//!
//! Implements `dossier`, `sbom`, `audit`, `evidence-package`. Outputs
//! byte-identical JSON to the Python reference (§8.2.h parity contract).

#![allow(dead_code)]

use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::ast::Program;
use crate::esk::attestation::{generate_dossier, generate_sbom};
use crate::esk::audit_engine::{FrameworkId, analyze_all, analyze_gaps};
// §Fase 118.b — the evidence packager writes a deterministic `.evidence.zip`,
// so it is the one part of the governance CLI that needs `zip`.
#[cfg(feature = "documents")]
use crate::esk::audit_engine::build_evidence_package;
use crate::ir_generator::IRGenerator;
use crate::ir_nodes::IRProgram;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::version::AXON_VERSION;

/// Two-space-indented, key-sorted JSON emission matching Python's
/// `json.dumps(..., indent=2, sort_keys=True)` with default `ensure_ascii=True`:
/// non-ASCII characters are escaped as `\uXXXX` for byte-identical parity
/// with the Python reference CLI output.
pub fn canonical_json(value: &Value) -> String {
    canonical_json_ensure_ascii(value, true)
}

/// Same as `canonical_json`, but lets the caller opt out of ASCII escaping
/// to match `json.dumps(..., ensure_ascii=False)` (used by `axon compile`
/// and `axon dossier`/`sbom` on the Python side).
pub fn canonical_json_utf8(value: &Value) -> String {
    canonical_json_ensure_ascii(value, false)
}

fn canonical_json_ensure_ascii(value: &Value, ensure_ascii: bool) -> String {
    let sorted = sort_value(value);
    let raw = serde_json::to_string_pretty(&sorted).expect("serialise");
    if ensure_ascii { escape_non_ascii(&raw) } else { raw }
}

/// Replace every non-ASCII scalar in a JSON string with `\uXXXX`,
/// matching Python's `json.dumps(..., ensure_ascii=True)` default.
/// Only runs inside already-quoted string literals (the JSON payload),
/// but since serde_json's pretty output never contains raw non-ASCII
/// outside string literals this is safe to apply to the whole output.
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
                // Surrogate pair encoding for code points > U+FFFF.
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
        Value::Object(map) => {
            let mut sorted = serde_json::Map::new();
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for k in keys {
                sorted.insert(k.clone(), sort_value(&map[k]));
            }
            Value::Object(sorted)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(sort_value).collect()),
        other => other.clone(),
    }
}

/// Compile a `.axon` source file into an IRProgram. Returns `None` on
/// parse or type-check failure after printing the diagnostics to stderr.
fn compile_file(file: &str) -> Result<IRProgram, i32> {
    let path = Path::new(file);
    if !path.exists() {
        eprintln!("X File not found: {}", file);
        return Err(2);
    }
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("X Cannot read {}: {e}", file);
            return Err(2);
        }
    };
    let tokens = match Lexer::new(&source, file).tokenize() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("X Lex error in {}: {}", file, e.message);
            return Err(1);
        }
    };
    let program: Program = match Parser::new(tokens).parse() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("X Parse error in {}: {}", file, e.message);
            return Err(1);
        }
    };
    use crate::type_checker::TypeChecker;
    let diagnostics = TypeChecker::new(&program).check();
    if !diagnostics.is_empty() {
        eprintln!("X {} has {} type error(s) — run 'axon check' for details.", file, diagnostics.len());
        return Err(1);
    }
    Ok(IRGenerator::new().generate(&program))
}

pub fn write_or_print(text: &str, output: Option<&str>, success_msg: &str) -> i32 {
    match output {
        Some(path) => match fs::write(path, text) {
            Ok(()) => {
                println!("OK {} {}", success_msg, path);
                0
            }
            Err(e) => {
                eprintln!("X write {}: {e}", path);
                2
            }
        },
        None => {
            println!("{text}");
            0
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
//  axon dossier
// ═══════════════════════════════════════════════════════════════════

pub fn run_dossier(file: &str, output: Option<&str>) -> i32 {
    let ir = match compile_file(file) {
        Ok(ir) => ir,
        Err(code) => return code,
    };
    let dossier = generate_dossier(&ir, AXON_VERSION);
    let text = canonical_json(&dossier.to_value());
    write_or_print(&text, output, "dossier written to")
}

// ═══════════════════════════════════════════════════════════════════
//  axon sbom
// ═══════════════════════════════════════════════════════════════════

pub fn run_sbom(file: &str, output: Option<&str>) -> i32 {
    let ir = match compile_file(file) {
        Ok(ir) => ir,
        Err(code) => return code,
    };
    let sbom = generate_sbom(&ir, AXON_VERSION);
    let text = canonical_json(&sbom.to_value());
    write_or_print(&text, output, "SBOM written to")
}

// ═══════════════════════════════════════════════════════════════════
//  axon audit
// ═══════════════════════════════════════════════════════════════════

pub fn run_audit(file: &str, framework: &str, output: Option<&str>) -> i32 {
    let ir = match compile_file(file) {
        Ok(ir) => ir,
        Err(code) => return code,
    };
    let payload: Value = match framework {
        "all" => {
            let analyses = analyze_all(&ir);
            let mut frameworks = serde_json::Map::new();
            let mut summary = serde_json::Map::new();
            for (name, a) in &analyses {
                frameworks.insert(name.clone(), a.to_value());
                let mut s = serde_json::Map::new();
                // Parity with Python audit_cmd.py — `readiness_percent` is the
                // full-precision float (no rounding), `ready` is the integer
                // count of ready controls (NOT a boolean predicate).
                s.insert("readiness_percent".into(), a.readiness_percent().into());
                s.insert("ready".into(), (a.ready as i64).into());
                s.insert("total".into(), (a.total_controls as i64).into());
                s.insert("pending_code".into(), (a.pending_code as i64).into());
                s.insert("pending_external".into(), (a.pending_external as i64).into());
                summary.insert(name.clone(), Value::Object(s));
            }
            let mut root = serde_json::Map::new();
            root.insert("schema".into(), "axon.esk.audit_gap_report.v1".into());
            root.insert("program".into(), Path::new(file).file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(file)
                .to_string()
                .into());
            root.insert("frameworks".into(), Value::Object(frameworks));
            root.insert("summary".into(), Value::Object(summary));
            Value::Object(root)
        }
        other => {
            let fw = match other {
                "soc2" => FrameworkId::Soc2TypeII,
                "iso27001" => FrameworkId::Iso27001,
                "fips" => FrameworkId::Fips140_3,
                "cc" => FrameworkId::CcEal4Plus,
                _ => {
                    eprintln!(
                        "X Unknown framework '{other}'. Use one of: soc2, iso27001, fips, cc, all."
                    );
                    return 2;
                }
            };
            let a = analyze_gaps(&ir, fw);
            let mut root = serde_json::Map::new();
            root.insert("schema".into(), "axon.esk.audit_gap_report.v1".into());
            root.insert("program".into(), Path::new(file).file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(file)
                .to_string()
                .into());
            root.insert("analysis".into(), a.to_value());
            Value::Object(root)
        }
    };
    let text = canonical_json(&payload);
    write_or_print(&text, output, "audit report written to")
}

// ═══════════════════════════════════════════════════════════════════
//  axon evidence-package
// ═══════════════════════════════════════════════════════════════════

// §Fase 118.b — `axon evidence-package` stays in `--help` under every profile
// and refuses in writing when the feature is absent. The §111 doctrine applied
// to packaging: the advertised surface stays advertised, and the refusal names
// the exact command that fixes it. A subcommand that quietly disappears from a
// build is the same defect §111 spent a fase removing.
#[cfg(not(feature = "documents"))]
pub fn run_evidence_package(_file: &str, _output: Option<&str>, _note: &str) -> i32 {
    eprintln!(
        "X `axon evidence-package` requires the `documents` feature — this build was compiled without it, so the deterministic .evidence.zip packager is absent.
  Reinstall with: cargo install axon-lang --features documents
  (`axon dossier`, `axon sbom` and `axon audit` work in this build — only the ZIP bundle needs the feature.)"
    );
    2
}

#[cfg(feature = "documents")]
pub fn run_evidence_package(file: &str, output: Option<&str>, note: &str) -> i32 {
    let ir = match compile_file(file) {
        Ok(ir) => ir,
        Err(code) => return code,
    };
    let source = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => String::new(),
    };
    let fname = Path::new(file)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(file)
        .to_string();
    let mut sources = std::collections::BTreeMap::new();
    sources.insert(fname, source);

    let pkg = build_evidence_package(&ir, AXON_VERSION, None, None, Some(sources), note);
    let out_path = match output {
        Some(p) => p.to_string(),
        None => {
            // Default <file>.evidence.zip next to the source.
            let p = Path::new(file);
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("program");
            let parent = p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
            if parent.is_empty() {
                format!("{stem}.evidence.zip")
            } else {
                format!("{parent}/{stem}.evidence.zip")
            }
        }
    };
    let path = pkg.write_zip(&out_path);
    let bytes = pkg.to_zip_bytes();
    println!(
        "OK evidence package written to {} ({} files, {} bytes)",
        path.display(),
        pkg.files.len(),
        bytes.len()
    );

    // ── §Fase 124.b — the DETACHED hybrid signature ──────────────────────
    //
    // `Ed25519(H) ‖ ML-DSA-65(H)` over the exact ZIP bytes just written.
    //
    // Detached, not inside the ZIP, because the packager's contract is
    // byte-identical output on equal inputs — and a hedged ML-DSA signature
    // plus per-run keypairs is nondeterministic by nature. Signing the ZIP
    // from outside preserves the determinism auditors diff against, and a
    // detached signature over the exact file bytes is also what an auditor's
    // tooling expects to consume.
    //
    // The keys are generated per package and their PUBLIC halves ship in the
    // sig file: this signature proves the package is intact SINCE PACKAGING,
    // not who packaged it. A durable signer identity needs key custody (§94)
    // and is named follow-up work in the §124 plan — claiming identity from
    // an ephemeral key would be the overclaim this fase exists to remove.
    #[cfg(feature = "csys-native")]
    {
        use crate::esk::hybrid_signer::{HybridSigner, HYBRID_SIGNATURE_BYTES};
        use crate::esk::provenance::Signer as _;

        fn hex(b: &[u8]) -> String {
            b.iter().map(|x| format!("{x:02x}")).collect()
        }

        let Some(signer) = HybridSigner::generate() else {
            // The ZIP exists and is fine; the SIGNATURE could not be made.
            // Failing the command is deliberate: exiting 0 here would let a
            // pipeline treat an unsigned package as a signed one, which is
            // the silent-substitution defect with legal evidence attached.
            eprintln!(
                "X the evidence package was written, but the OS RNG failed and no hybrid \
                 signature could be produced. Refusing to report success for an unsigned \
                 package — re-run to sign."
            );
            return 2;
        };
        let sig = signer.sign(&bytes);
        debug_assert_eq!(sig.len(), HYBRID_SIGNATURE_BYTES);
        let keys = signer.public_keys();

        // Self-check before publishing: a signature file that does not verify
        // against the bytes beside it is worse than none.
        if !HybridSigner::verify_hybrid(&bytes, &sig, &keys) {
            eprintln!("X hybrid signature self-check failed — not writing a signature that does not verify");
            return 2;
        }

        let sig_json = serde_json::json!({
            "schema": "axon.esk.evidence_signature.v1",
            "algorithm": signer.algorithm(),
            "signs": "the ZIP file bytes, exactly as written",
            "zip_sha256": hex(&axon_csys::crypto::sha256(&bytes)),
            "ed25519_public_key": hex(&keys.ed25519),
            "ml_dsa_65_public_key": hex(&keys.ml_dsa_65),
            "signature": hex(&sig),
            "note": "Keys are generated per package; this proves integrity since packaging, not signer identity. Durable identity requires key custody (§94 integration, planned).",
        });
        let sig_path = format!("{}.sig.json", path.display());
        if let Err(e) = fs::write(&sig_path, serde_json::to_string_pretty(&sig_json).unwrap_or_default()) {
            eprintln!("X could not write {sig_path}: {e}");
            return 2;
        }
        println!("OK hybrid signature (Ed25519+ML-DSA-65 over SHA-256) written to {sig_path}");
    }
    #[cfg(not(feature = "csys-native"))]
    println!(
        "~ hybrid signature skipped: this build lacks `csys-native` (the C cryptographic \
         boundary). The package is complete and unsigned."
    );

    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_json_sorts_and_indents() {
        let v = serde_json::json!({"b": 1, "a": {"z": 2, "y": 3}});
        let s = canonical_json(&v);
        assert!(s.starts_with('{'));
        // Keys sorted alphabetically.
        let a_pos = s.find("\"a\"").unwrap();
        let b_pos = s.find("\"b\"").unwrap();
        assert!(a_pos < b_pos);
        // Nested keys sorted too.
        let y_pos = s.find("\"y\"").unwrap();
        let z_pos = s.find("\"z\"").unwrap();
        assert!(y_pos < z_pos);
    }

    /// §Fase 124.b — **the production door of the hybrid signature.**
    ///
    /// The `HybridSigner`'s own unit tests prove the construction; this proves
    /// the WIRING — that `axon evidence-package` actually emits a detached
    /// signature an external party can verify against the ZIP bytes it sits
    /// beside, using only what the sig file itself carries. The whole §122
    /// line exists because engines pass their own tests while no production
    /// path reaches them; this is the path.
    #[cfg(all(feature = "documents", feature = "csys-native"))]
    #[test]
    fn evidence_package_ships_a_verifiable_detached_hybrid_signature() {
        use crate::esk::hybrid_signer::{HybridPublicKeys, HybridSigner};

        fn unhex(s: &str) -> Vec<u8> {
            (0..s.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
                .collect()
        }

        let dir = std::env::temp_dir().join(format!("axon_124b_sig_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let src_path = dir.join("p.axon");
        std::fs::write(&src_path, "flow F() -> Unit { step S { ask: \"go\" } }\n").unwrap();
        let out = dir.join("p.evidence.zip");

        let code = run_evidence_package(
            src_path.to_str().unwrap(),
            Some(out.to_str().unwrap()),
            "firma hibrida, camino de produccion",
        );
        assert_eq!(code, 0, "the packager must succeed end to end");

        let zip = std::fs::read(&out).expect("the ZIP must exist");
        let sig_path = format!("{}.sig.json", out.display());
        let sig_file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&sig_path).expect(
                "the detached signature must exist beside the ZIP — a signer with no \
                 production caller is the defect this fase exists to close",
            ))
            .expect("valid JSON");

        assert_eq!(sig_file["algorithm"], "Ed25519+ML-DSA-65");

        // Verify EXACTLY as an external auditor would: from the sig file's own
        // fields, against the file bytes on disk, no access to the signer.
        let keys = HybridPublicKeys {
            ed25519: unhex(sig_file["ed25519_public_key"].as_str().unwrap())
                .try_into()
                .expect("32-byte Ed25519 public key"),
            ml_dsa_65: unhex(sig_file["ml_dsa_65_public_key"].as_str().unwrap())
                .try_into()
                .expect("1952-byte ML-DSA-65 public key"),
        };
        let sig = unhex(sig_file["signature"].as_str().unwrap());
        assert!(
            HybridSigner::verify_hybrid(&zip, &sig, &keys),
            "the detached signature must verify against the ZIP bytes using only what the \
             sig file carries"
        );

        // And the property the signature exists for: any altered byte in the
        // package must break it.
        let mut tampered = zip.clone();
        tampered[zip.len() / 2] ^= 1;
        assert!(
            !HybridSigner::verify_hybrid(&tampered, &sig, &keys),
            "a tampered package must not verify"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
