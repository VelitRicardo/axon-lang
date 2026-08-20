//! The adopter surface speaks versions and T-codes — never internal phase
//! numbers.
//!
//! An outside reviewer installed this crate, read the README and the messages
//! the runtime printed, and reported back the internal development-phase
//! numbering it found there. Development phases are internal planning
//! context; a version (`since 2.3.0`), a diagnostic code (`axon-T1215`) or a
//! named theorem (`Theorem 5.1`) is what an adopter can act on. This test is
//! the gate that keeps the two vocabularies apart.
//!
//! # What counts as the adopter surface
//!
//! Everything an adopter reads WITHOUT opening the compiler's source:
//!
//! - every `README.md`, `CONTRIBUTING.md`, `DEVELOPMENT.md`, `CLA.md`;
//! - every crate `Cargo.toml` and `build.rs` (anyone who unpacks the crate);
//! - `axon --help` — the `///` doc comments on the clap items in `main.rs`;
//! - **every string literal** in shipped `src/` trees — a runtime error, a
//!   deploy warning, a CLI line is adopter-visible by definition. Assert
//!   messages inside `#[cfg(test)]` modules ride along: the scanner cannot
//!   tell them apart and the cost of keeping them clean is nil;
//! - `examples/`, `packaging/`, `infrastructure/`, `scripts/`, `project/` —
//!   whole files. NOT `migrations/`: a shipped sqlx migration is checksummed
//!   and therefore immutable (see the body).
//!
//! In the monorepo the scan is WHOLE-FILE over every shipped `src/`, the test
//! suites and their fixtures (file and directory names included — a
//! `faseNNN_*.rs` travels in the crate), the C kernels, benches, build scripts,
//! the CI workflows and the MCP server crate. The literal-only scanner remains
//! for the packaged-crate layout, where only `src/` is present.
//!
//! # What is banned
//!
//! - the section sign `§` — outright, including for paper sections, so the
//!   gate needs no heuristic ("Section 5.3 of the CSP paper" is the public
//!   spelling);
//! - the word `Fase` / `fase` (whole word, or as the `faseNNN` prefix);
//! - internal decision ids of the shape `D119.5`.
//!
//! # Layouts
//!
//! In the monorepo (`../README.md` exists next to this crate) the FULL surface
//! is mandatory and a missing file is a failure, not a skip — a gate that
//! silently scans nothing is the defect it exists to catch. In the packaged
//! crate (no monorepo root) only the crate-local surface is scanned.

use std::fs;
use std::path::{Path, PathBuf};

// ───────────────────────────── banned vocabulary ─────────────────────────────

/// One hit: where, and the line that carried it.
#[derive(Debug)]
struct Hit {
    path: PathBuf,
    line: usize,
    excerpt: String,
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `Fase` / `fase` as a whole word, or as the `faseNNN` file-name prefix
/// (`fase119_m1_…`). Rejects `phase`, `faser`, `Fasermuster`.
fn has_fase_word(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i + 4 <= b.len() {
        if (b[i] == b'F' || b[i] == b'f')
            && &b[i + 1..i + 4] == b"ase"
            && (i == 0 || !is_word_byte(b[i - 1]))
        {
            let after = b.get(i + 4).copied();
            match after {
                None => return true,
                Some(c) if !is_word_byte(c) => return true,
                Some(c) if c.is_ascii_digit() => return true, // fase119_…
                _ => {}
            }
        }
        i += 1;
    }
    false
}

/// `D` + two or three digits + `.` + digit, as a whole token: `D119.5`,
/// `D124.4`, `D7.x` does NOT match (one digit) and neither does `MD5.1`.
fn has_decision_id(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'D' && (i == 0 || !is_word_byte(b[i - 1])) {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let digits = j - (i + 1);
            if (2..=3).contains(&digits)
                && j + 1 < b.len()
                && b[j] == b'.'
                && b[j + 1].is_ascii_digit()
            {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn banned(s: &str) -> bool {
    s.contains('§') || has_fase_word(s) || has_decision_id(s)
}

// ───────────────────────────── scanners ─────────────────────────────

/// Whole-file scan: every line is adopter-visible.
fn scan_whole_file(path: &Path, hits: &mut Vec<Hit>) {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("adopter-surface gate: cannot read {}: {e}", path.display()));
    for (n, line) in text.lines().enumerate() {
        if banned(line) {
            hits.push(Hit { path: path.to_path_buf(), line: n + 1, excerpt: line.trim().to_string() });
        }
    }
}

/// Extract the contents of every string literal in a Rust (or C) source
/// file, with the 1-based line each one STARTS on. A small state machine:
/// skips `//` line comments and `/* */` block comments; understands `"…"`
/// with backslash escapes, Rust raw strings `r"…"` / `r#"…"#` / `br#"…"#`,
/// byte strings `b"…"`, and char literals (so `'"'` does not open a string).
fn string_literals(text: &str) -> Vec<(usize, String)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1;
    let n = b.len();
    let bump = |c: u8, line: &mut usize| {
        if c == b'\n' {
            *line += 1;
        }
    };
    while i < n {
        let c = b[i];
        // line comment
        if c == b'/' && i + 1 < n && b[i + 1] == b'/' {
            while i < n && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // block comment
        if c == b'/' && i + 1 < n && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < n && !(b[i] == b'*' && b[i + 1] == b'/') {
                bump(b[i], &mut line);
                i += 1;
            }
            i += 2;
            continue;
        }
        // raw string: r"…", r#"…"#, br"…", br#…
        if (c == b'r' || (c == b'b' && i + 1 < n && b[i + 1] == b'r'))
            && (i == 0 || !is_word_byte(b[i - 1]))
        {
            let mut j = if c == b'b' { i + 2 } else { i + 1 };
            let mut hashes = 0;
            while j < n && b[j] == b'#' {
                hashes += 1;
                j += 1;
            }
            if j < n && b[j] == b'"' {
                let start_line = line;
                j += 1;
                let body_start = j;
                loop {
                    if j >= n {
                        break;
                    }
                    if b[j] == b'"' {
                        let mut k = 0;
                        while k < hashes && j + 1 + k < n && b[j + 1 + k] == b'#' {
                            k += 1;
                        }
                        if k == hashes {
                            break;
                        }
                    }
                    bump(b[j], &mut line);
                    j += 1;
                }
                out.push((start_line, String::from_utf8_lossy(&b[body_start..j.min(n)]).into_owned()));
                i = (j + 1 + hashes).min(n);
                continue;
            }
        }
        // char literal / lifetime: skip `'x'`, `'\''`, `'\n'`; leave `'a` lifetimes alone
        if c == b'\'' {
            if i + 2 < n && b[i + 1] == b'\\' {
                // escaped char: '\n', '\'', '\u{…}'
                let mut j = i + 2;
                while j < n && b[j] != b'\'' && b[j] != b'\n' {
                    j += 1;
                }
                i = j + 1;
                continue;
            }
            if i + 2 < n && b[i + 2] == b'\'' {
                i += 3;
                continue;
            }
            i += 1;
            continue;
        }
        // ordinary (or byte) string
        if c == b'"' {
            let start_line = line;
            let mut j = i + 1;
            let body_start = j;
            while j < n && b[j] != b'"' {
                if b[j] == b'\\' {
                    j += 1;
                }
                if j < n {
                    bump(b[j], &mut line);
                }
                j += 1;
            }
            out.push((start_line, String::from_utf8_lossy(&b[body_start..j.min(n)]).into_owned()));
            i = j + 1;
            continue;
        }
        bump(c, &mut line);
        i += 1;
    }
    out
}

/// Source scan: string literals only (plus, when `doc_comments` is set, the
/// `///` and `//!` lines — clap turns those into `--help` text).
fn scan_source(path: &Path, doc_comments: bool, hits: &mut Vec<Hit>) {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("adopter-surface gate: cannot read {}: {e}", path.display()));
    for (line, lit) in string_literals(&text) {
        if banned(&lit) {
            let excerpt: String = lit.chars().take(160).collect();
            hits.push(Hit { path: path.to_path_buf(), line, excerpt: format!("\"{excerpt}\"") });
        }
    }
    if doc_comments {
        for (n, l) in text.lines().enumerate() {
            let t = l.trim_start();
            if (t.starts_with("///") || t.starts_with("//!")) && banned(t) {
                hits.push(Hit { path: path.to_path_buf(), line: n + 1, excerpt: t.to_string() });
            }
        }
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let rd = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("adopter-surface gate: cannot list {}: {e}", dir.display()));
    for entry in rd {
        let p = entry.expect("dir entry").path();
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if p.is_dir() {
            if matches!(name, "target" | "node_modules" | ".git" | ".terraform" | "vendor") {
                continue;
            }
            assert!(
                !banned(name),
                "adopter-surface gate: directory `{}` is named with internal phase vocabulary",
                p.display()
            );
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn has_ext(p: &Path, exts: &[&str]) -> bool {
    p.extension().and_then(|e| e.to_str()).map(|e| exts.contains(&e)).unwrap_or(false)
}

// ───────────────────────────── the surface ─────────────────────────────

/// The only files that may carry the banned vocabulary: the gates that exist to
/// REFUSE it, and therefore have to spell it out. A gate cannot enforce a
/// pattern it is forbidden to name.
///
/// This list is deliberately two entries long and each one is justified. It is
/// NOT a place to park a file that merely happens to mention a cycle number —
/// the sweep that widened this scan to whole files renamed every `faseNNN_*`
/// test and fixture and rewrote every comment, so the carve-out that used to sit
/// here for `advertised.rs` is gone and should stay gone.
const ENFORCERS: &[&str] = &[
    // this gate: the vocabulary it bans appears in `banned()` and in its own
    // self-tests, which assert that `§`, `Fase` and a decision id are caught.
    "adopter_surface_speaks_versions.rs",
    // the docs-tree gate: `internal_shape()` matches the NAMES the internal
    // material uses (`fase*`, `vision*`, `presentation_*`), so it must contain
    // them as literals, and its probe paths name `documents/fase/…`.
    "the_public_docs_tree_is_public.rs",
];

fn must_exist(p: &Path) -> &Path {
    assert!(p.exists(), "adopter-surface gate: expected {} to exist — the surface list is out of date, not the repo", p.display());
    p
}

#[test]
fn the_adopter_surface_carries_no_phase_numbers() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = crate_dir.parent().map(Path::to_path_buf).unwrap_or_else(|| crate_dir.clone());
    let monorepo = root.join("README.md").exists() && root.join("axon-frontend").is_dir();

    let mut hits = Vec::new();

    // 1. documents and manifests — whole files
    let mut docs: Vec<PathBuf> = vec![crate_dir.join("README.md"), crate_dir.join("Cargo.toml")];
    if monorepo {
        for rel in ["README.md", "CONTRIBUTING.md", "DEVELOPMENT.md", "CLA.md"] {
            docs.push(root.join(rel));
        }
        for krate in ["axon-frontend", "axon-csys", "axon-agora"] {
            docs.push(root.join(krate).join("README.md"));
            docs.push(root.join(krate).join("Cargo.toml"));
        }
        docs.push(root.join("axon-csys").join("build.rs"));
    }
    for d in &docs {
        scan_whole_file(must_exist(d), &mut hits);
    }

    // 2. data / example / ops directories — whole files.
    //
    // `migrations/` is deliberately NOT here: `sqlx::migrate!` checksums every
    // applied migration and refuses to start against a database whose recorded
    // checksum differs, so a migration that has shipped is immutable — comment
    // text included. `003_add_tenants.sql` carries seven phase references that
    // will outlive this gate for exactly that reason; new migrations are held to
    // the rule by review.
    let mut dirs: Vec<PathBuf> = Vec::new();
    if monorepo {
        for rel in ["examples", "packaging", "infrastructure", "scripts", "project"] {
            dirs.push(root.join(rel));
        }
    }
    for d in &dirs {
        let mut files = Vec::new();
        walk(must_exist(d), &mut files);
        for f in files {
            // binaries (images, archives) are not text surface
            if has_ext(&f, &["png", "jpg", "jpeg", "gif", "ico", "zip", "gz", "exe", "wasm", "pdf"]) {
                continue;
            }
            if fs::read_to_string(&f).is_err() {
                continue; // non-UTF-8 blob
            }
            scan_whole_file(&f, &mut hits);
        }
    }

    // 3. shipped source, tests, C kernels, benches, build scripts and the CI
    //    workflows — WHOLE files. Comments included: an adopter who opens the
    //    crate reads them, docs.rs renders the `///` ones, and a test file name
    //    travels in the package. The string-literal-only scanner that used to
    //    run here is kept (below) for the packaged-crate layout, where only
    //    `src/` ships; in the monorepo every byte is on the surface.
    let mut whole_trees: Vec<PathBuf> = vec![crate_dir.join("src"), crate_dir.join("tests")];
    if monorepo {
        for krate in ["axon-frontend", "axon-csys", "axon-agora"] {
            for sub in ["src", "tests", "benches", "c-src", "tools"] {
                let p = root.join(krate).join(sub);
                if p.is_dir() {
                    whole_trees.push(p);
                }
            }
            let b = root.join(krate).join("build.rs");
            if b.exists() {
                whole_trees.push(b);
            }
        }
        whole_trees.push(root.join(".github"));
        whole_trees.push(root.join("src").join("axon-emcp"));
        whole_trees.push(root.join("tests"));
    }
    for tree in &whole_trees {
        let mut files = Vec::new();
        if tree.is_file() {
            files.push(tree.clone());
        } else {
            walk(must_exist(tree), &mut files);
        }
        for f in files {
            if f
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| ENFORCERS.contains(&n))
            {
                continue;
            }
            if !has_ext(&f, &["rs", "c", "h", "yml", "yaml", "toml", "md", "axon", "axi", "json", "pinned", "js", "sh", "ps1", "txt"]) {
                continue;
            }
            // the file NAME is surface too (a `faseNNN_*.rs` travels in the crate)
            if let Some(name) = f.file_name().and_then(|n| n.to_str()) {
                if banned(name) {
                    hits.push(Hit { path: f.clone(), line: 0, excerpt: format!("(file name) {name}") });
                }
            }
            if fs::read_to_string(&f).is_err() {
                continue;
            }
            scan_whole_file(&f, &mut hits);
        }
    }
    if !monorepo {
        // packaged crate: the string-literal scanner over src/ (comments are
        // swept upstream; this layout only needs to catch a message regression)
        let main_rs = crate_dir.join("src").join("main.rs");
        let mut files = Vec::new();
        walk(must_exist(&crate_dir.join("src")), &mut files);
        for f in files {
            if has_ext(&f, &["rs"]) {
                scan_source(&f, f == main_rs, &mut hits);
            }
        }
    }

    if !hits.is_empty() {
        hits.sort_by(|a, b| (a.path.clone(), a.line).cmp(&(b.path.clone(), b.line)));
        let mut report = String::new();
        for h in &hits {
            let rel = h.path.strip_prefix(&root).unwrap_or(&h.path);
            report.push_str(&format!("  {}:{}: {}\n", rel.display(), h.line, h.excerpt));
        }
        panic!(
            "adopter-surface gate: {} line(s) carry internal phase vocabulary (`§`, `Fase`, `D<nn>.<n>`).\n\
             The adopter surface speaks versions and T-codes. Rewrite each one — the phase number was \
             provenance; the version it shipped in, or nothing, is the public spelling.\n{report}",
            hits.len()
        );
    }
}

// ───────────────────────────── the scanner is itself tested ─────────────────────────────

#[test]
fn banned_vocabulary_is_recognised_and_nothing_else_is() {
    assert!(banned("see §119.m.1 for why"));
    assert!(banned("Fase 41 adds sockets"));
    assert!(banned("gate: fase119_m1_agent_executor.rs"));
    assert!(banned("ratified D124.4"));
    assert!(banned("(D119.5, ratified 2026-08-08)"));
    assert!(!banned("since 2.3.0, axon-T1215 rejects it"));
    assert!(!banned("phase two of the rollout"));
    assert!(!banned("Theorem 5.1 ceiling"));
    assert!(!banned("MD5.1 is not a decision id"));
    assert!(!banned("D7.x one digit is not a decision id"));
    assert!(!banned("HIPAA section 164.312 is the public spelling"));
}

#[test]
fn section_sign_is_banned_even_for_paper_sections() {
    assert!(banned("HIPAA § 164.312"));
    assert!(banned("CSP §5.3: tools as constraint satisfaction"));
}

#[test]
fn string_literal_scanner_sees_literals_and_not_comments() {
    let src = r##"
// §Fase 1 — a comment, not surface
/* §Fase 2 block comment */
fn f() {
    let a = "plain §3";
    let b = r#"raw "quoted" §4"#;
    let c = b"bytes §5";
    let d = '"'; // char literal must not open a string §6
    let e = "escaped \" quote §7";
    let f = "multi
line §8";
    let g = "clean";
}
"##;
    let lits = string_literals(src);
    let bodies: Vec<&str> = lits.iter().map(|(_, s)| s.as_str()).collect();
    assert_eq!(
        bodies,
        vec!["plain §3", "raw \"quoted\" §4", "bytes §5", "escaped \\\" quote §7", "multi\nline §8", "clean"],
        "{lits:?}"
    );
    // the multi-line literal is reported on the line it STARTS
    assert_eq!(lits[4].0, 10, "{lits:?}");
    assert!(!bodies.iter().any(|b| b.contains("§1") || b.contains("§2") || b.contains("§6")));
}
