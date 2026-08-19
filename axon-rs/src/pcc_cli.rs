//! v2.4.0 — `axon pcc` CLI: prove + verify.
//!
//! Two subcommands close the Proof-Carrying Code loop at the command
//! line:
//!
//! - `axon pcc prove <source.axon>` — compile the source, generate a
//!   [`ProofBundle`] across all five property classes, emit it as JSON.
//! - `axon pcc verify <source.axon> <bundle.json>` — recompile the
//!   source (an INDEPENDENT re-derivation of the artifact) and check
//!   every proof in the bundle against it. Exit 0 iff every proof
//!   `Verified`; exit 1 if any is `Refuted` / `DigestMismatch` /
//!   `UnknownProperty`.
//!
//! The verifier recompiles the source itself rather than trusting a
//! supplied IR: the artifact a proof binds to is "the IR THIS source
//! compiles to," and the digest binding catches a bundle minted
//! for different source. The verdict logic is the small, auditable
//! [`crate::pcc::checker`] — the trust boundary a third party reviews.

#![allow(dead_code)]

use std::fs;
use std::path::Path;

use crate::audit_cli::{canonical_json_utf8, write_or_print};
use crate::ir_generator::IRGenerator;
use crate::ir_nodes::IRProgram;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::pcc::{check_bundle, generate_all_proofs, CheckOutcome, ProofBundle};
use crate::version::AXON_VERSION;

/// Compile a `.axon` file to its IR, or return a process exit code on
/// failure. Mirrors `audit_cli::compile_file` (lex → parse →
/// type-check → IR) so `prove` + `verify` derive the artifact the same
/// way every other CLI surface does.
fn compile_file(file: &str) -> Result<IRProgram, i32> {
    let path = Path::new(file);
    if !path.exists() {
        eprintln!("X File not found: {file}");
        return Err(2);
    }
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("X Cannot read {file}: {e}");
            return Err(2);
        }
    };
    let tokens = match Lexer::new(&source, file).tokenize() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("X Lex error in {file}: {}", e.message);
            return Err(1);
        }
    };
    let program = match Parser::new(tokens).parse() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("X Parse error in {file}: {}", e.message);
            return Err(1);
        }
    };
    use crate::type_checker::TypeChecker;
    let diagnostics = TypeChecker::new(&program).check();
    if !diagnostics.is_empty() {
        eprintln!(
            "X {file} has {} type error(s) — run 'axon check' for details.",
            diagnostics.len()
        );
        return Err(1);
    }
    Ok(IRGenerator::new().generate(&program))
}

/// `axon pcc prove <file>` — emit the proof bundle for `file`.
pub fn run_pcc_prove(file: &str, output: Option<&str>) -> i32 {
    let ir = match compile_file(file) {
        Ok(ir) => ir,
        Err(code) => return code,
    };
    let proofs = generate_all_proofs(&ir, AXON_VERSION);
    let bundle = ProofBundle {
        axon_version: AXON_VERSION.to_string(),
        artifact_digest: crate::pcc::artifact_digest(&ir),
        proofs,
    };
    let value = match serde_json::to_value(&bundle) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("X failed to serialize proof bundle: {e}");
            return 2;
        }
    };
    let text = canonical_json_utf8(&value);
    write_or_print(&text, output, "PCC proof bundle written to")
}

/// `axon pcc verify <file> <bundle>` — recompile `file`, check every
/// proof in `bundle` against it, print per-proof verdicts + a summary,
/// exit 0 iff all verified.
pub fn run_pcc_verify(file: &str, bundle_path: &str) -> i32 {
    let ir = match compile_file(file) {
        Ok(ir) => ir,
        Err(code) => return code,
    };
    let bundle_text = match fs::read_to_string(bundle_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("X Cannot read proof bundle {bundle_path}: {e}");
            return 2;
        }
    };
    let bundle: ProofBundle = match serde_json::from_str(&bundle_text) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("X Malformed proof bundle {bundle_path}: {e}");
            return 2;
        }
    };

    if bundle.proofs.is_empty() {
        println!("PCC verify: bundle declares no proofs for {file} — nothing to verify (OK).");
        return 0;
    }

    // v2.4.0 — dogfood the trusted aggregate: the "deployable?" predicate
    // (all_verified) lives in the checker, not here. This CLI and
    // the enterprise deploy gate render the SAME `BundleReport`.
    let report = check_bundle(&bundle, &ir);
    let total = report.results.len();
    let mut verified = 0usize;
    for r in &report.results {
        let slug = r.property.slug();
        let subject = &r.subject;
        match &r.outcome {
            CheckOutcome::Verified => {
                verified += 1;
                println!("  OK   [{slug}] {subject} — verified");
            }
            CheckOutcome::Refuted { reason } => {
                println!("  FAIL [{slug}] {subject} — refuted: {reason}");
            }
            CheckOutcome::DigestMismatch => {
                println!(
                    "  FAIL [{slug}] {subject} — digest mismatch: this proof is not for {file}"
                );
            }
            CheckOutcome::UnknownProperty => {
                println!(
                    "  FAIL [{slug}] {subject} — unknown property (checker does not understand this proof shape)"
                );
            }
        }
    }

    if report.all_verified() {
        println!("PCC verify: {verified}/{total} proofs VERIFIED against {file}.");
        0
    } else {
        let failed = total - verified;
        println!("PCC verify: {failed}/{total} proofs FAILED against {file} ({verified} verified).");
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a unique temp path for a test (no extra deps; per-test
    /// filename avoids cross-test collision under parallel runs).
    fn temp_path(stem: &str, ext: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("axon_pcc_cli_{stem}.{ext}"))
    }

    /// Roundtrip on empty source: prove emits a bundle with zero
    /// proofs (no certifiable subjects), verify exits 0.
    #[test]
    fn prove_then_verify_empty_source_roundtrips() {
        let src = temp_path("empty", "axon");
        let bundle = temp_path("empty", "json");
        fs::write(&src, "").expect("write source");

        let prove_code =
            run_pcc_prove(src.to_str().unwrap(), Some(bundle.to_str().unwrap()));
        assert_eq!(prove_code, 0, "prove should succeed on empty source");

        let verify_code =
            run_pcc_verify(src.to_str().unwrap(), bundle.to_str().unwrap());
        assert_eq!(verify_code, 0, "verify should pass an empty bundle");

        let _ = fs::remove_file(&src);
        let _ = fs::remove_file(&bundle);
    }

    /// A tampered bundle (a proof claiming a subject + digest not from
    /// this source) makes verify exit 1.
    #[test]
    fn verify_tampered_bundle_exits_one() {
        let src = temp_path("tamper", "axon");
        let bundle = temp_path("tamper", "json");
        fs::write(&src, "").expect("write source");

        // A bundle with a bogus proof bound to a digest that cannot
        // match the empty-source IR.
        let bogus = r#"{
            "axon_version": "test",
            "artifact_digest": "deadbeef",
            "proofs": [
                {
                    "property": "ComplianceCoverage",
                    "artifact_digest": "deadbeef",
                    "witness": {
                        "kind": "ComplianceCoverage",
                        "endpoint_name": "Ghost",
                        "required_classes": ["HIPAA"],
                        "shield_ref": "",
                        "shield_present": false,
                        "provided_classes": [],
                        "unknown_classes": [],
                        "uncovered_classes": ["HIPAA"]
                    },
                    "axon_version": "test"
                }
            ]
        }"#;
        fs::write(&bundle, bogus).expect("write bundle");

        let verify_code =
            run_pcc_verify(src.to_str().unwrap(), bundle.to_str().unwrap());
        assert_eq!(verify_code, 1, "verify must reject a tampered bundle");

        let _ = fs::remove_file(&src);
        let _ = fs::remove_file(&bundle);
    }

    /// A malformed bundle file makes verify exit 2 (operational error,
    /// distinct from a proof failure).
    #[test]
    fn verify_malformed_bundle_exits_two() {
        let src = temp_path("malformed", "axon");
        let bundle = temp_path("malformed", "json");
        fs::write(&src, "").expect("write source");
        fs::write(&bundle, "{ not json").expect("write bundle");

        let verify_code =
            run_pcc_verify(src.to_str().unwrap(), bundle.to_str().unwrap());
        assert_eq!(verify_code, 2, "malformed bundle is an operational error");

        let _ = fs::remove_file(&src);
        let _ = fs::remove_file(&bundle);
    }

    /// Missing source file → exit 2.
    #[test]
    fn prove_missing_file_exits_two() {
        assert_eq!(
            run_pcc_prove("Z:/definitely/missing.axon", None),
            2,
            "missing source is an operational error"
        );
    }
}
