//! v2.0.0 — `axon fmt` subcommand (Rust binary parity).
//!
//! Token-level round-trip formatter. Direct port of the Python
//! `axon.compiler.formatter.format_source` (v1.5.2 MVP).
//!
//! The algorithm walks every token (effective + comment) emitted by
//! the lexer in source order, re-emits each at its original
//! `(line, column)` position, padding with newlines + spaces to
//! preserve layout. Comments survive verbatim (regular, outer doc,
//! inner doc — line + block). The final output is right-trimmed per
//! line + ends with exactly one `\n`.
//!
//! ## Idempotence
//!
//! `format_source(format_source(x)) == format_source(x)` for every
//! input. The MVP intentionally preserves the author's existing
//! layout; canonicalisation rules (indent width, brace style) are
//! deferred to a future cycle — same scope as the Python MVP.
//!
//! ## Why a token-level formatter
//!
//! The lexer's lossless channel (v1.5.2) already records every
//! comment with its exact `(line, column)`. Combined with the
//! effective tokens, the source byte stream can be reconstructed
//! deterministically. This is enough to call `axon fmt --check` a
//! lossless contract: if the formatter changes nothing beyond the
//! documented cosmetic normalisation, the file is canonical.

use axon_frontend::lexer::Lexer;
use axon_frontend::tokens::{Token, TokenType};

/// v2.0.0 — re-render a lexer token to source form. The lexer
/// strips delimiters from certain token kinds (string literals
/// store only their content sans `"..."`); the formatter MUST
/// re-add them to produce re-lexable output.
fn render_token_to_source(tok: &Token) -> String {
    match tok.ttype {
        TokenType::StringLit => {
            // Re-quote with proper escape sequences for backslash +
            // double-quote + newline + tab. Match the lexer's
            // `scan_string` escape set verbatim.
            let mut out = String::with_capacity(tok.value.len() + 2);
            out.push('"');
            for c in tok.value.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '"' => out.push_str("\\\""),
                    '\n' => out.push_str("\\n"),
                    '\t' => out.push_str("\\t"),
                    other => out.push(other),
                }
            }
            out.push('"');
            out
        }
        _ => tok.value.clone(),
    }
}

/// Format an AXON source string. Returns the canonicalised source
/// or a `LexerError` message when the input doesn't tokenise.
///
/// Cosmetic normalisations applied:
///   - every line right-trimmed of trailing whitespace
///   - file ends with exactly one `\n`
///
/// Beyond those two, the output is byte-identical to the input
/// modulo position re-emission.
pub fn format_source(src: &str) -> Result<String, String> {
    let tokens = Lexer::new(src, "<fmt>")
        .tokenize()
        .map_err(|e| format!("lex error at {}:{}: {}", e.line, e.column, e.message))?;

    let mut pieces: Vec<String> = Vec::new();
    let mut cur_line: u32 = 1;
    let mut cur_col: u32 = 1;

    for tok in tokens {
        // Skip EOF sentinel.
        if matches!(tok.ttype, axon_frontend::tokens::TokenType::Eof) {
            continue;
        }

        // Catch up to the token's line by emitting newlines.
        if tok.line > cur_line {
            for _ in 0..(tok.line - cur_line) {
                pieces.push("\n".to_string());
            }
            cur_line = tok.line;
            cur_col = 1;
        }

        // Catch up to the token's column by emitting spaces.
        if tok.column > cur_col {
            for _ in 0..(tok.column - cur_col) {
                pieces.push(" ".to_string());
            }
            cur_col = tok.column;
        }

        // v2.0.0 — re-render token to source form. The lexer
        // strips delimiters from string literals (`"..."` → just
        // the content), so the formatter MUST re-add them to
        // produce re-lexable output. Same defensive shape for
        // other token kinds whose value is sans-delimiter.
        let rendered = render_token_to_source(&tok);
        pieces.push(rendered.clone());

        // Block comments can contain newlines — update the cursor
        // accordingly so the next token positions correctly.
        if rendered.contains('\n') {
            let parts: Vec<&str> = rendered.split('\n').collect();
            cur_line += (parts.len() - 1) as u32;
            cur_col = parts.last().map(|s| s.len() as u32).unwrap_or(0) + 1;
        } else {
            cur_col += rendered.chars().count() as u32;
        }
    }

    let raw: String = pieces.join("");
    // Right-trim every line, collapse trailing blank lines to a
    // single final \n.
    let trimmed: String = raw
        .split('\n')
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    let mut result = trimmed.trim_end_matches('\n').to_string();
    result.push('\n');
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_empty_source_returns_newline() {
        // Edge case: empty source → just a final \n.
        let out = format_source("").expect("empty source lexes");
        assert_eq!(out, "\n");
    }

    #[test]
    fn fmt_trailing_whitespace_trimmed() {
        let src = "persona Alice   \n";
        let out = format_source(src).expect("lexes");
        assert!(!out.contains("Alice   "));
        assert!(out.contains("Alice"));
    }

    #[test]
    fn fmt_multiple_trailing_newlines_collapsed() {
        let src = "persona Alice\n\n\n\n";
        let out = format_source(src).expect("lexes");
        assert!(out.ends_with("\n"));
        // Collapsed to single final \n.
        assert!(!out.ends_with("\n\n"));
    }

    #[test]
    fn fmt_idempotent_on_well_formed_source() {
        let src = "persona Alice {\n  confidence_threshold: 0.85\n}\n";
        let once = format_source(src).expect("first pass");
        let twice = format_source(&once).expect("second pass");
        assert_eq!(once, twice, "format_source MUST be idempotent");
    }

    #[test]
    fn fmt_line_comment_preserved() {
        let src = "persona Alice {\n  // a comment\n}\n";
        let out = format_source(src).expect("lexes");
        assert!(out.contains("// a comment"));
    }

    #[test]
    fn fmt_block_comment_preserved() {
        let src = "persona Alice {\n  /* block */\n}\n";
        let out = format_source(src).expect("lexes");
        assert!(out.contains("/* block */"));
    }

    #[test]
    fn fmt_doc_line_comment_preserved() {
        let src = "/// outer doc\npersona Alice {\n}\n";
        let out = format_source(src).expect("lexes");
        assert!(out.contains("/// outer doc"));
    }

    #[test]
    fn fmt_inner_doc_preserved() {
        let src = "//! inner doc\npersona Alice {\n}\n";
        let out = format_source(src).expect("lexes");
        assert!(out.contains("//! inner doc"));
    }

    #[test]
    fn fmt_lex_error_returns_err() {
        // Unterminated string literal — should fail to lex.
        let src = "persona Alice { name: \"unclosed\n";
        let r = format_source(src);
        assert!(r.is_err());
    }

    #[test]
    fn fmt_check_mode_well_formed_returns_unchanged() {
        // The --check mode (handled by main.rs) compares
        // formatted vs original. For an already-formatted source,
        // the diff is empty.
        let src = "persona Alice {\n  confidence_threshold: 0.85\n}\n";
        let out = format_source(src).expect("lexes");
        // Either byte-identical OR differs only in whitespace
        // canonicalization (trailing newlines / line-trim).
        assert_eq!(out.trim_end(), src.trim_end());
    }
}
