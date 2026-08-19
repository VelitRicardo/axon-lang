//! v2.77.0 — the drift gate for the axon-agora authorization
//! laws, as a single-implementation golden corpus driven through the REAL
//! pipeline (`ems::compile_project`).
//!
//! Each `tests/fixtures/laws/<case>/` holds an `main.axon` (+ any
//! module files) and an `expected.json`. The runner compiles the entry through
//! the EMS, isolates the diagnostics carrying the case's `law` code, and pins
//! them EXACT on `code` + `file` + `line` + `column`, with the message matched
//! by ORDERED substring anchors that must include the named subject and the fix
//! (the contract, not the bytes — byte-goldens rot; the v2.67.0 nine-placeholder
//! scar). Diagnostics of OTHER laws are ignored, so an unrelated warning never
//! destabilises this corpus.
//!
//! Adding a case is adding a directory. A change in the law's line/column or a
//! dropped anchor fails here, at the source, not in an adopter's incident.

use std::path::{Path, PathBuf};

use axon_frontend::ems::{compile_project, EmsDiagnostic, EmsOptions};

/// One expected diagnostic, pinned exact but message-by-anchors.
struct ExpectedDiagnostic {
    code: String,
    file: String,
    line: u32,
    column: u32,
    anchors: Vec<String>,
}

struct ExpectedCase {
    law: String,
    diagnostics: Vec<ExpectedDiagnostic>,
}

/// A tiny hand-rolled reader for the fixed `expected.json` shape (the frontend
/// carries `serde_json`, but a dependency-light reader keeps the fixture schema
/// legible and the failure messages precise).
fn parse_expected(path: &Path) -> ExpectedCase {
    let raw = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let v: serde_json::Value =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    let law = v["law"].as_str().expect("`law` string").to_string();
    let diagnostics = v["diagnostics"]
        .as_array()
        .expect("`diagnostics` array")
        .iter()
        .map(|d| ExpectedDiagnostic {
            code: d["code"].as_str().expect("`code`").to_string(),
            file: d["file"].as_str().expect("`file`").to_string(),
            line: d["line"].as_u64().expect("`line`") as u32,
            column: d["column"].as_u64().expect("`column`") as u32,
            anchors: d["message_anchors"]
                .as_array()
                .expect("`message_anchors`")
                .iter()
                .map(|a| a.as_str().expect("anchor string").to_string())
                .collect(),
        })
        .collect();
    ExpectedCase { law, diagnostics }
}

/// Compile the case entry and return ALL diagnostics (errors + warnings),
/// whichever way `compile_project` resolved.
fn all_diagnostics(entry: &Path) -> Vec<EmsDiagnostic> {
    let opts = EmsOptions { modules_root: None, use_cache: false, cache_dir: None };
    match compile_project(entry, &opts) {
        Ok(s) => s.warnings,
        Err(f) => f.errors.into_iter().chain(f.warnings).collect(),
    }
}

/// The diagnostic's file as a machine-independent BASENAME (the EMS origin is
/// an absolute path; the golden pins only the file name).
fn basename(file: &str) -> String {
    Path::new(file)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| file.to_string())
}

/// Ordered-anchor substring match: each anchor must appear, in order, later
/// than the previous — so the message keeps its shape, without byte-freezing it.
fn message_has_ordered_anchors(message: &str, anchors: &[String]) -> Result<(), String> {
    let mut from = 0usize;
    for anchor in anchors {
        match message[from..].find(anchor.as_str()) {
            Some(pos) => from += pos + anchor.len(),
            None => {
                return Err(format!(
                    "anchor {anchor:?} missing (or out of order) in message:\n  {message}"
                ))
            }
        }
    }
    Ok(())
}

fn run_case(dir: &Path) {
    let name = dir.file_name().unwrap().to_string_lossy().to_string();
    let expected = parse_expected(&dir.join("expected.json"));
    let actual = all_diagnostics(&dir.join("main.axon"));

    // Isolate the diagnostics of THIS law (by its code prefix in the message —
    // `TypeError` carries the code as a string prefix, the house style).
    let mut actual_law: Vec<&EmsDiagnostic> =
        actual.iter().filter(|d| d.message.contains(&expected.law)).collect();

    assert_eq!(
        actual_law.len(),
        expected.diagnostics.len(),
        "[{name}] expected {} {}-diagnostic(s), got {}:\n{}",
        expected.diagnostics.len(),
        expected.law,
        actual_law.len(),
        actual_law
            .iter()
            .map(|d| format!("  {}:{}:{} {}", d.file, d.line, d.column, d.message))
            .collect::<Vec<_>>()
            .join("\n")
    );

    // Each expected diagnostic must be matched by exactly one actual (order-
    // independent: match by code+file+line+column, then anchors).
    for exp in &expected.diagnostics {
        let pos = actual_law.iter().position(|d| {
            d.message.contains(&exp.code)
                && basename(&d.file) == exp.file
                && d.line == exp.line
                && d.column == exp.column
        });
        let idx = pos.unwrap_or_else(|| {
            panic!(
                "[{name}] no actual {} diagnostic at {}:{}:{}. Actual {}-diagnostics:\n{}",
                exp.code,
                exp.file,
                exp.line,
                exp.column,
                expected.law,
                actual_law
                    .iter()
                    .map(|d| format!("  {}:{}:{}", d.file, d.line, d.column))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        });
        let d = actual_law.remove(idx);
        if let Err(e) = message_has_ordered_anchors(&d.message, &exp.anchors) {
            panic!("[{name}] {}: {e}", exp.code);
        }
    }
}

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/laws")
}

#[test]
fn every_fase116_law_fixture_matches_its_golden() {
    let root = fixtures_root();
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("read fixtures {}: {e}", root.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "no fixtures under {}", root.display());
    for dir in &cases {
        run_case(dir);
    }
    // A count pin: the corpus must not silently shrink (a deleted case is a
    // dropped guarantee — the v2.67.0 no-silent-cap posture).
    assert!(cases.len() >= 3, "the v2.77.0 corpus lost cases: {}", cases.len());
}
