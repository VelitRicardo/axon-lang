//! `axon repl` native implementation — interactive Read-Eval-Print Loop.
//!
//! Provides an interactive session where users can type AXON declarations
//! and see them lexed, parsed, type-checked, and compiled to IR in real-time.
//!
//! Features:
//!   - Multi-line input (detects open braces and waits for closing)
//!   - Real-time Lex → Parse → TypeCheck → IR pipeline
//!   - Dot-commands: .help, .clear, .quit
//!   - Error recovery without session crash
//!
//! Exit codes:
//!   0 — normal exit

use std::io::{self, BufRead, IsTerminal, Write};

use crate::ir_generator::IRGenerator;
use crate::lexer::{Lexer, LexerError};
use crate::parser::{ParseError, Parser};
use crate::version::AXON_VERSION;
use crate::type_checker::TypeChecker;

// ── ANSI colors ─────────────────────────────────────────────────────────────

const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

fn c(text: &str, code: &str, use_color: bool) -> String {
    if use_color {
        format!("{code}{text}{RESET}")
    } else {
        text.to_string()
    }
}

// ── Banner ──────────────────────────────────────────────────────────────────

fn print_banner(use_color: bool) {
    if use_color {
        println!("{CYAN}\u{2554}{}\u{2557}{RESET}", "\u{2550}".repeat(42));
        println!("{CYAN}\u{2551}{RESET}  {BOLD}{GREEN}AXON REPL{RESET}  v{AXON_VERSION}                   {CYAN}\u{2551}{RESET}");
        println!("{CYAN}\u{2551}{RESET}  A cognitive language for AI              {CYAN}\u{2551}{RESET}");
        println!("{CYAN}\u{255a}{}\u{255d}{RESET}", "\u{2550}".repeat(42));
        println!("  Type {YELLOW}.help{RESET} for commands, {YELLOW}.quit{RESET} to exit.");
    } else {
        println!("+{}+", "=".repeat(42));
        println!("|  AXON REPL  v{AXON_VERSION}                   |");
        println!("|  A cognitive language for AI              |");
        println!("+{}+", "=".repeat(42));
        println!("  Type .help for commands, .quit to exit.");
    }
    println!();
}

// ── Dot-commands ────────────────────────────────────────────────────────────

/// Handle a dot-command. Returns `Some(true)` to continue, `Some(false)` to exit,
/// `None` if this wasn't a dot-command.
fn handle_dot_command(cmd: &str, use_color: bool) -> Option<bool> {
    let cmd = cmd.trim().to_lowercase();
    if !cmd.starts_with('.') {
        return None;
    }

    match cmd.as_str() {
        ".quit" | ".exit" | ".q" => {
            println!("{}", c("Goodbye.", DIM, use_color));
            Some(false)
        }
        ".help" => {
            println!();
            println!("  {}      Show this message", c(".help", YELLOW, use_color));
            println!("  {}     Clear screen", c(".clear", YELLOW, use_color));
            println!("  {}      Exit REPL", c(".quit", YELLOW, use_color));
            println!();
            Some(true)
        }
        ".clear" => {
            print!("\x1b[2J\x1b[H");
            let _ = io::stdout().flush();
            Some(true)
        }
        _ => {
            println!("{}", c(&format!("  Unknown command: {cmd}. Type .help"), RED, use_color));
            Some(true)
        }
    }
}

// ── Eval pipeline ───────────────────────────────────────────────────────────

fn eval_source(source: &str, use_color: bool) {
    // Lex
    let tokens = match Lexer::new(source, "<repl>").tokenize() {
        Ok(t) => t,
        Err(LexerError { message, .. }) => {
            eprintln!("{}", c(&format!("  Lexer error: {message}"), RED, use_color));
            return;
        }
    };

    // Parse
    let mut parser = Parser::new(tokens);
    let program = match parser.parse() {
        Ok(p) => p,
        Err(ParseError { message, line, .. }) => {
            let loc = if line > 0 { format!(" (line {line})") } else { String::new() };
            eprintln!("{}", c(&format!("  Parse error{loc}: {message}"), RED, use_color));
            return;
        }
    };

    // Type-check (non-fatal: display warnings but continue)
    let type_errors = TypeChecker::new(&program).check();
    for te in &type_errors {
        let loc = if te.line > 0 { format!(" (line {})", te.line) } else { String::new() };
        eprintln!("{}", c(&format!("  Type error{loc}: {}", te.message), YELLOW, use_color));
    }

    // IR generation
    let ir_program = IRGenerator::new().generate(&program);

    // Serialize and display
    let ir_value = serde_json::to_value(&ir_program).unwrap_or(serde_json::Value::Null);
    let formatted = serde_json::to_string_pretty(&ir_value).unwrap_or_default();
    println!("{}", c(&formatted, GREEN, use_color));
}

// ── Multi-line input ────────────────────────────────────────────────────────

fn read_multiline(first_line: &str, reader: &mut dyn BufRead, use_color: bool) -> String {
    let mut lines = vec![first_line.to_string()];
    let mut depth: i32 = first_line.matches('{').count() as i32
        - first_line.matches('}').count() as i32;

    while depth > 0 {
        print!("{} ", c("  ...", DIM, use_color));
        let _ = io::stdout().flush();

        let mut cont = String::new();
        match reader.read_line(&mut cont) {
            Ok(0) => return String::new(), // EOF
            Ok(_) => {
                let trimmed = cont.trim_end_matches('\n').trim_end_matches('\r');
                depth += trimmed.matches('{').count() as i32
                    - trimmed.matches('}').count() as i32;
                lines.push(trimmed.to_string());
            }
            Err(_) => return String::new(),
        }
    }

    lines.join("\n")
}

// ── Public entry point ──────────────────────────────────────────────────────

pub fn run_repl() -> i32 {
    let use_color = io::stdout().is_terminal() && io::stdin().is_terminal();
    let stdin = io::stdin();
    let mut reader = stdin.lock();

    print_banner(use_color);

    loop {
        print!("{} ", c("axon>", &format!("{CYAN}{BOLD}"), use_color));
        let _ = io::stdout().flush();

        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => {
                // EOF
                println!("{}", c("\nGoodbye.", DIM, use_color));
                return 0;
            }
            Ok(_) => {}
            Err(_) => {
                println!("{}", c("\nGoodbye.", DIM, use_color));
                return 0;
            }
        }

        let stripped = line.trim();
        if stripped.is_empty() {
            continue;
        }

        // Dot-commands
        if let Some(should_continue) = handle_dot_command(stripped, use_color) {
            if !should_continue {
                return 0;
            }
            continue;
        }

        // Multi-line detection
        let source = if stripped.contains('{')
            && stripped.matches('{').count() > stripped.matches('}').count()
        {
            let s = read_multiline(stripped, &mut reader, use_color);
            if s.is_empty() {
                continue;
            }
            s
        } else {
            stripped.to_string()
        };

        eval_source(&source, use_color);
    }
}

// ── Testable helpers ────────────────────────────────────────────────────────

/// Evaluate AXON source and return (ir_json, type_errors, had_parse_error).
/// Used by integration tests.
pub fn eval_source_captured(source: &str) -> Result<(String, Vec<String>), String> {
    // Lex
    let tokens = Lexer::new(source, "<repl>")
        .tokenize()
        .map_err(|e| format!("Lexer error: {}", e.message))?;

    // Parse
    let mut parser = Parser::new(tokens);
    let program = parser
        .parse()
        .map_err(|e| format!("Parse error: {}", e.message))?;

    // Type-check
    let type_errors: Vec<String> = TypeChecker::new(&program)
        .check()
        .iter()
        .map(|te| te.message.clone())
        .collect();

    // IR generation
    let ir_program = IRGenerator::new().generate(&program);
    let ir_value = serde_json::to_value(&ir_program).unwrap_or(serde_json::Value::Null);
    let formatted = serde_json::to_string_pretty(&ir_value).unwrap_or_default();

    Ok((formatted, type_errors))
}
