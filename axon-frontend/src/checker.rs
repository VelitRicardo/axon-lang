//! `axon check` native implementation.
//!
//! Pipeline for C6:
//!   1. Read file (exit 2 if not found)
//!   2. Lex → token list (exit 1 on lexer error)
//!   3. Parse → AST (exit 1 on parse error)
//!   4. Type check → errors (exit 1 on type errors)
//!   5. Count tokens and declarations from AST
//!   6. Report result — format matches Python check_cmd output

use std::io::{self, IsTerminal};
use std::path::Path;

use crate::ast::Declaration;
use crate::lexer::{Lexer, LexerError};
use crate::parser::{ParseError, Parser};
use crate::type_checker::TypeChecker;

// ── ANSI color helpers ────────────────────────────────────────────────────────

struct Colors {
    green_bold: &'static str,
    red_bold: &'static str,
    yellow_bold: &'static str,
    bold: &'static str,
    dim: &'static str,
    reset: &'static str,
}

impl Colors {
    fn new(enabled: bool) -> Self {
        if enabled {
            Colors {
                green_bold: "\x1b[1;32m",
                red_bold: "\x1b[1;31m",
                yellow_bold: "\x1b[1;33m",
                bold: "\x1b[1m",
                dim: "\x1b[2m",
                reset: "\x1b[0m",
            }
        } else {
            Colors {
                green_bold: "",
                red_bold: "",
                yellow_bold: "",
                bold: "",
                dim: "",
                reset: "",
            }
        }
    }
}

// ── Declaration counter ──────────────────────────────────────────────────────

fn count_declarations(decls: &[Declaration]) -> usize {
    let mut count = 0;
    for decl in decls {
        count += 1;
        if let Declaration::Epistemic(eb) = decl {
            count += count_declarations(&eb.body);
        }
    }
    count
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Run `axon check` natively. Returns an exit code (0 / 1 / 2).
///
/// `strict = true` (v1.6.0 D4) promotes warnings (e.g. legacy
/// string-topic listeners) to errors so the check exits non-zero.
pub fn run_check(
    file: &str,
    no_color: bool,
    strict: bool,
    schemas_dir: Option<&str>,
) -> i32 {
    let use_color = !no_color && io::stdout().is_terminal();
    let c = Colors::new(use_color);

    let path = Path::new(file);
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.to_string());

    // ── 1. Read source ────────────────────────────────────────────
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("{}X File not found: {}{}", c.red_bold, file, c.reset);
            return 2;
        }
    };

    // ── 1.b v2.76.0 — the EMS engages when the entry declares any
    // import. One statement means module semantics were requested; the
    // refused forms then refuse loudly instead of staying
    // decorative as they did through v2.75.0.
    if crate::ems::source_declares_imports(&source, file) {
        let manifest_owned = match load_manifest(schemas_dir, &filename, &c) {
            Ok(m) => m,
            Err(code) => return code,
        };
        return run_check_ems(path, &filename, strict, manifest_owned.as_ref(), &c);
    }

    // ── 2. Lex ───────────────────────────────────────────────────
    let tokens = match Lexer::new(&source, file).tokenize() {
        Ok(t) => t,
        Err(LexerError {
            message,
            line,
            column,
        }) => {
            let loc = if column > 0 {
                format!(":{line}:{column}")
            } else {
                format!(":{line}")
            };
            eprintln!("{}X {filename}{loc}{}  {message}", c.red_bold, c.reset);
            return 1;
        }
    };

    // ── 3. Token count ───────────────────────────────────────────
    let token_count = tokens.len();

    // ── 4. Parse → AST ───────────────────────────────────────────
    let mut parser = Parser::new(tokens);
    let program = match parser.parse() {
        Ok(p) => p,
        Err(ParseError {
            message,
            line,
            column,
            ..
        }) => {
            let loc = if column > 0 {
                format!(":{line}:{column}")
            } else {
                format!(":{line}")
            };
            eprintln!(
                "{}X {filename}{loc}{}  Parse error: {message}",
                c.red_bold, c.reset
            );
            return 1;
        }
    };

    // ── 5. Declaration count from AST ────────────────────────────
    let declaration_count = count_declarations(&program.declarations);

    // ── 6. Type check ────────────────────────────────────────────
    // v1.31.0 (D2, D3) — when `--schemas-dir <path>` is supplied
    // (or `AXON_SCHEMAS_DIR` env var), load + merge every
    // `.axon-schema.json` under the path and feed the resulting
    // `Manifest` to the type-checker. T801-T805 + T803 run against
    // forms (b) `manifest_ref` and (c) `env_var` exactly as they do
    // for form (a) inline. Without the flag, behavior is byte-
    // identical to v1.38.3 (D5 backwards-compat absolute).
    let manifest_owned = match load_manifest(schemas_dir, &filename, &c) {
        Ok(m) => m,
        Err(code) => return code,
    };
    let (type_errors, type_warnings) = match &manifest_owned {
        Some(m) => TypeChecker::with_manifest(&program, m).check_with_warnings(),
        None => TypeChecker::new(&program).check_with_warnings(),
    };

    if !type_errors.is_empty() {
        eprintln!(
            "{}X {filename}{}  {} error(s){}",
            c.red_bold,
            c.reset,
            type_errors.len(),
            if type_warnings.is_empty() {
                String::new()
            } else {
                format!(", {} warning(s)", type_warnings.len())
            }
        );
        for te in &type_errors {
            eprintln!("  error [line {}]: {}", te.line, te.message);
        }
        for tw in &type_warnings {
            eprintln!("  warning [line {}]: {}", tw.line, tw.message);
        }
        return 1;
    }

    // ── 6.b v1.6.0 — strict mode promotes warnings to errors ─
    if strict && !type_warnings.is_empty() {
        eprintln!(
            "{}X {filename}{}  0 errors, {} warning(s) {}(--strict){}",
            c.red_bold,
            c.reset,
            type_warnings.len(),
            c.red_bold,
            c.reset,
        );
        for tw in &type_warnings {
            eprintln!("  error [line {}]: {}", tw.line, tw.message);
        }
        return 1;
    }

    // ── 7. Report (warnings present but non-strict — pass with hint) ─
    if !type_warnings.is_empty() {
        println!(
            "{}\u{26A0}{} {}{filename}{}  {}{token_count} tokens \u{00B7} {declaration_count} declarations \u{00B7} 0 errors \u{00B7} {} warning(s){}",
            c.yellow_bold, c.reset,
            c.bold, c.reset,
            c.dim, type_warnings.len(), c.reset,
        );
        for tw in &type_warnings {
            println!("  warning [line {}]: {}", tw.line, tw.message);
        }
        return 0;
    }

    // ── 7.b. Fully clean ────────────────────────────────────────
    println!(
        "{}\u{2713}{} {}{filename}{}  {}{token_count} tokens \u{00B7} {declaration_count} declarations \u{00B7} 0 errors{}",
        c.green_bold, c.reset,
        c.bold, c.reset,
        c.dim, c.reset,
    );

    0
}

/// v1.31.0 — load + merge the `--schemas-dir` manifests (shared by
/// the single-file path and the v2.76.0 EMS path). `Err(exit_code)` when the
/// directory fails to load.
fn load_manifest(
    schemas_dir: Option<&str>,
    filename: &str,
    c: &Colors,
) -> Result<Option<crate::store_schema_manifest::Manifest>, i32> {
    match schemas_dir {
        Some(path) if !path.trim().is_empty() => {
            match crate::store_schema_manifest::load_and_merge_manifests(Path::new(path)) {
                Ok(m) => Ok(Some(m)),
                Err(e) => {
                    eprintln!(
                        "{}X {filename}{}  schemas-dir load error: {e}",
                        c.red_bold, c.reset
                    );
                    Err(1)
                }
            }
        }
        _ => Ok(None),
    }
}

/// v2.76.0 — `axon check` over a multi-module project. Same exit
/// contract as the single-file path (0 clean · 1 diagnostics); the
/// report gains a module count, and every diagnostic names its FILE
/// (module-local line) because the compilation spans several.
fn run_check_ems(
    entry: &Path,
    filename: &str,
    strict: bool,
    manifest: Option<&crate::store_schema_manifest::Manifest>,
    c: &Colors,
) -> i32 {
    let opts = crate::ems::EmsOptions {
        modules_root: std::env::var("AXON_MODULES_ROOT").ok().map(Into::into),
        use_cache: true,
        cache_dir: None,
    };

    let base = |origin: &str| -> String {
        Path::new(origin)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| origin.to_string())
    };

    match crate::ems::compile_project_with_manifest(entry, &opts, manifest) {
        Err(fail) => {
            eprintln!(
                "{}X {filename}{}  {} error(s){}",
                c.red_bold,
                c.reset,
                fail.errors.len(),
                if fail.warnings.is_empty() {
                    String::new()
                } else {
                    format!(", {} warning(s)", fail.warnings.len())
                }
            );
            for e in &fail.errors {
                eprintln!("  error [{} line {}]: {}", base(&e.file), e.line, e.message);
            }
            for w in &fail.warnings {
                eprintln!("  warning [{} line {}]: {}", base(&w.file), w.line, w.message);
            }
            1
        }
        Ok(out) => {
            if strict && !out.warnings.is_empty() {
                eprintln!(
                    "{}X {filename}{}  0 errors, {} warning(s) {}(--strict){}",
                    c.red_bold,
                    c.reset,
                    out.warnings.len(),
                    c.red_bold,
                    c.reset,
                );
                for w in &out.warnings {
                    eprintln!("  error [{} line {}]: {}", base(&w.file), w.line, w.message);
                }
                return 1;
            }
            if !out.warnings.is_empty() {
                println!(
                    "{}\u{26A0}{} {}{filename}{}  {}{} tokens \u{00B7} {} declarations \u{00B7} {} modules \u{00B7} 0 errors \u{00B7} {} warning(s){}",
                    c.yellow_bold, c.reset,
                    c.bold, c.reset,
                    c.dim,
                    out.token_count,
                    out.declaration_count,
                    out.module_count,
                    out.warnings.len(),
                    c.reset,
                );
                for w in &out.warnings {
                    println!("  warning [{} line {}]: {}", base(&w.file), w.line, w.message);
                }
                return 0;
            }
            println!(
                "{}\u{2713}{} {}{filename}{}  {}{} tokens \u{00B7} {} declarations \u{00B7} {} modules \u{00B7} 0 errors{}",
                c.green_bold, c.reset,
                c.bold, c.reset,
                c.dim,
                out.token_count,
                out.declaration_count,
                out.module_count,
                c.reset,
            );
            0
        }
    }
}
