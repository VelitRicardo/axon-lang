//! `axon compile` native implementation.
//!
//! Pipeline: Source → Lex → Parse → Type-check → IR Generate → JSON

use std::io::{self, IsTerminal};
use std::path::Path;

use crate::ir_generator::IRGenerator;
use crate::lexer::{Lexer, LexerError};
use crate::parser::{ParseError, Parser};
use crate::version::AXON_VERSION;
use crate::type_checker::TypeChecker;

// ── ANSI helpers ─────────────────────────────────────────────────────────────

fn c(text: &str, code: &str, use_color: bool) -> String {
    if use_color {
        format!("{code}{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

// ── Public entry point ───────────────────────────────────────────────────────

pub fn run_compile(
    file: &str,
    backend: &str,
    output: Option<&str>,
    stdout: bool,
) -> i32 {
    let use_color = io::stdout().is_terminal();
    let path = Path::new(file);
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.to_string());

    // ── 1. Read source ───────────────────────────────────────────
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => {
            eprintln!(
                "{}",
                c(&format!("X File not found: {file}"), "\x1b[1;31m", use_color)
            );
            return 2;
        }
    };

    // ── 1.b v2.76.0 — a source with imports compiles through the
    // EMS: resolve → interfaces → ECC → link → ONE IR over the linked
    // program (module provenance included). The emitted JSON is the
    // deployable multi-module artifact.
    if axon_frontend::ems::source_declares_imports(&source, file) {
        let opts = axon_frontend::ems::EmsOptions {
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
        return match axon_frontend::ems::compile_project(path, &opts) {
            Err(fail) => {
                eprintln!(
                    "{}  {} error(s)",
                    c(&format!("X {filename}"), "\x1b[1;31m", use_color),
                    fail.errors.len()
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
                for w in &out.warnings {
                    eprintln!("  warning [{} line {}]: {}", base(&w.file), w.line, w.message);
                }
                emit_ir_json(&out.ir, file, backend, output, stdout, use_color)
            }
        };
    }

    // ── 2. Lex ───────────────────────────────────────────────────
    let tokens = match Lexer::new(&source, file).tokenize() {
        Ok(t) => t,
        Err(LexerError { message, line, column }) => {
            let loc = if column > 0 {
                format!(":{line}:{column}")
            } else {
                format!(":{line}")
            };
            eprintln!(
                "{}  {message}",
                c(&format!("X {filename}{loc}"), "\x1b[1;31m", use_color)
            );
            return 1;
        }
    };

    // ── 3. Parse ─────────────────────────────────────────────────
    let mut parser = Parser::new(tokens);
    let program = match parser.parse() {
        Ok(p) => p,
        Err(ParseError { message, line, column, .. }) => {
            let loc = if column > 0 {
                format!(":{line}:{column}")
            } else {
                format!(":{line}")
            };
            eprintln!(
                "{}  {message}",
                c(&format!("X {filename}{loc}"), "\x1b[1;31m", use_color)
            );
            return 1;
        }
    };

    // ── 4. Type check ────────────────────────────────────────────
    let type_errors = TypeChecker::new(&program).check();
    if !type_errors.is_empty() {
        eprintln!(
            "{}  {} type error(s)",
            c(&format!("X {filename}"), "\x1b[1;31m", use_color),
            type_errors.len()
        );
        for te in &type_errors {
            eprintln!("  error [line {}]: {}", te.line, te.message);
        }
        return 1;
    }

    // ── 5. Generate IR ───────────────────────────────────────────
    let ir_program = IRGenerator::new().generate(&program);

    // ── 6+7. Serialize + emit (shared with the v2.76.0 EMS path) ────
    emit_ir_json(&ir_program, file, backend, output, stdout, use_color)
}

/// Steps 6–7 of `axon compile`: attach `_meta`, serialize, and write to
/// stdout or the output file. Shared by the single-file path and the
/// v2.76.0 multi-module path (one emission, one shape).
fn emit_ir_json(
    ir_program: &axon_frontend::ir_nodes::IRProgram,
    file: &str,
    backend: &str,
    output: Option<&str>,
    stdout: bool,
    use_color: bool,
) -> i32 {
    let mut ir_value = serde_json::to_value(ir_program).unwrap_or(serde_json::Value::Null);

    // Add _meta
    if let serde_json::Value::Object(ref mut map) = ir_value {
        let meta = serde_json::json!({
            "source": file,
            "backend": backend,
            "axon_version": AXON_VERSION,
        });
        map.insert("_meta".to_string(), meta);
    }

    let ir_json = serde_json::to_string_pretty(&ir_value).unwrap_or_default();

    if stdout {
        println!("{ir_json}");
    } else {
        let out_path = match output {
            Some(o) => o.to_string(),
            None => {
                let p = Path::new(file).with_extension("ir.json");
                p.to_string_lossy().into_owned()
            }
        };
        if let Err(e) = std::fs::write(&out_path, &ir_json) {
            eprintln!("axon: failed to write {out_path}: {e}");
            return 2;
        }
        println!(
            "{}",
            c(&format!("\u{2713} Compiled \u{2192} {out_path}"), "\x1b[1;32m", use_color)
        );
    }

    0
}
