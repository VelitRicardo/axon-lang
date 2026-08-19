//! Native tool executors — Calculator and DateTimeTool.
//!
//! Tools are pure functions that execute locally (no LLM call).
//! When a `use_tool` step references a known tool, the runner
//! intercepts it and calls the executor directly.
//!
//! Supported tools:
//!   - Calculator: safe arithmetic expression evaluator
//!   - DateTimeTool: current date/time/timestamp queries

use std::time::{SystemTime, UNIX_EPOCH};

/// Result of a tool execution.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub tool_name: String,
}

/// v2.53.0 — the closed set of stdlib tool names that have a REAL native
/// executor here. Distinct from `stdlib::TOOLS`, which also lists LLM-backed
/// tools (e.g. `WebSearch`) that legitimately fall through to the model. A name
/// present in `stdlib::TOOLS` but NOT in this set AND not implemented below is
/// the pre-existing defect (v2.53.0 section 8): it silently degrades into the model
/// *hallucinating* the tool's output. `DocumentRenderer` (whose whole premise
/// is attestable, non-invented artifacts) MUST NOT ship into that behaviour.
const NATIVE_EXECUTOR_TOOLS: &[&str] = &[
    "Calculator",
    "DateTimeTool",
    "DocumentRenderer",
    // v2.54.0 — Document Ingestion & Surgical Edit.
    "DocumentReader",
    "DocumentEditor",
    // v2.54.0 — Inferred Ingestion: the extraction tools become real dispatch
    // arms. With no engine registered they typed-refuse (`no_engine_configured`),
    // NEVER fall through to the model inventing the document's contents.
    "PDFExtractor",
    "ImageTextExtractor",
    "ImageAnalyzer",
];

/// Dispatch a tool call by name. Returns `None` if the tool is not a native
/// executor AND is not a declared-but-unimplemented stdlib tool (those legacy
/// names still fall through to the LLM — see `dispatch_or_reject` for the
/// stricter contract the runtime uses at real call sites).
pub fn dispatch(tool_name: &str, argument: &str) -> Option<ToolResult> {
    match tool_name {
        "Calculator" => Some(calculator_execute(argument)),
        "DateTimeTool" => Some(datetime_execute(argument)),
        // v2.53.0 — Native Document Synthesis: render a `document` IR + bound
        // values into deterministic OOXML bytes (the design decision: returns the artifact
        // value, never a filesystem write).
        "DocumentRenderer" => Some(document_render_execute(argument)),
        // v2.54.0 — read an ingested OOXML document into a bounded, born-
        // Untrusted, Parsed text tree.
        "DocumentReader" => Some(document_read_execute(argument)),
        // v2.54.0 — surgical edit: touch only the targeted parts, emit the
        // per-part hash manifest, inherit taint.
        "DocumentEditor" => Some(document_edit_execute(argument)),
        // v2.54.0 — Inferred Ingestion. Each routes to the registered
        // extraction engine (the sidecar client v2.54.0 / the enterprise engine
        // v2.54.0); with none registered, each returns a TYPED refusal, never a
        // hallucinated read. OCR (`image:text`) and vision
        // (`image:description`) are distinct transforms.
        "PDFExtractor" => Some(extraction_execute("pdf:text", argument)),
        "ImageTextExtractor" => Some(extraction_execute("image:text", argument)),
        "ImageAnalyzer" => Some(extraction_execute("image:description", argument)),
        _ => None, // Not a native tool — fall through to LLM
    }
}

/// v2.53.0 section 8 — the honest dispatch: a tool name DECLARED in `stdlib::TOOLS`
/// but with no native executor and no LLM provider is a **typed refusal**, not
/// a silent hand-off to the model. This closes the pre-existing defect where
/// calling e.g. `PDFExtractor` (declared, unimplemented) returned the model
/// *inventing* the PDF's contents, shaped exactly like a real extraction.
/// Returns `Err` for such names; `Ok(Some)` for a real native result;
/// `Ok(None)` for a name with a legitimate non-native (LLM/provider) path.
pub fn dispatch_or_reject(tool_name: &str, argument: &str) -> Result<Option<ToolResult>, String> {
    if let Some(r) = dispatch(tool_name, argument) {
        return Ok(Some(r));
    }
    // Declared in the stdlib catalog but not natively executable here.
    let declared = crate::stdlib::TOOLS.iter().any(|t| t.name == tool_name);
    if declared && !NATIVE_EXECUTOR_TOOLS.contains(&tool_name) {
        // Only tools with a real provider (`requires_api_key` / a `provider`)
        // may legitimately reach a non-native backend; a declared tool with
        // neither a native executor nor a provider would silently become model
        // invention, so refuse.
        let has_provider = crate::stdlib::TOOLS
            .iter()
            .find(|t| t.name == tool_name)
            .map(|t| !t.provider.is_empty() || t.requires_api_key)
            .unwrap_or(false);
        if !has_provider {
            return Err(format!(
                "tool '{tool_name}' is declared in the stdlib catalog but has no native \
                 implementation and no provider — refusing to silently hand the call to the \
                 model, which would fabricate its output. Implement the tool, give \
                 it a provider, or remove it from the catalog."
            ));
        }
    }
    Ok(None)
}

/// v2.53.0 — render a `document` into OOXML bytes. The argument is JSON:
/// `{ "document": <IRDocument>, "values": { "<ref>": "<resolved text>" } }`.
/// Returns a typed artifact descriptor (sha256 + content type + base64 bytes)
/// as JSON — the HOST decides where the bytes land.
// v2.81.0 — the OOXML surface lives behind the `documents` feature. The
// tool STAYS REGISTERED and refuses in writing: a capability that silently
// vanishes from a build is the v2.67.0 defect (an advertised primitive that is not
// there), and a runtime that answers "unknown tool" teaches the adopter nothing.
#[cfg(not(feature = "documents"))]
fn document_render_execute(_argument: &str) -> ToolResult {
    ToolResult {
        success: false,
        output: "DocumentRenderer: this build was compiled without the `documents` feature, so the OOXML engine is absent. Reinstall with: cargo install axon-lang --features documents"
            .to_string(),
        tool_name: "DocumentRenderer".to_string(),
    }
}

#[cfg(feature = "documents")]
fn document_render_execute(argument: &str) -> ToolResult {
    use base64::Engine;
    let name = "DocumentRenderer";
    let parsed: serde_json::Value = match serde_json::from_str(argument) {
        Ok(v) => v,
        Err(e) => return err(name, format!("invalid render request JSON: {e}")),
    };
    let doc_val = match parsed.get("document") {
        Some(d) => d.clone(),
        None => return err(name, "render request missing `document`".into()),
    };
    let spec: crate::ooxml::DocumentSpec = match serde_json::from_value(doc_val) {
        Ok(s) => s,
        Err(e) => return err(name, format!("malformed document IR: {e}")),
    };
    let values: std::collections::BTreeMap<String, String> = parsed
        .get("values")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    match crate::ooxml::render(&spec, &values) {
        Ok(out) => {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&out.bytes);
            let descriptor = serde_json::json!({
                "sha256": out.sha256_hex,
                "content_type": out.content_type,
                "extension": out.extension,
                "size_bytes": out.bytes.len(),
                "bytes_base64": b64,
            });
            ToolResult {
                success: true,
                output: descriptor.to_string(),
                tool_name: name.to_string(),
            }
        }
        Err(e) => err(name, e.to_string()),
    }
}

/// v2.54.0 — the shared extraction dispatch arm. `transform` is the OTS
/// transform the calling tool asked for (`pdf:text` / `image:text` /
/// `image:description`). The argument is JSON: `{ "bytes_base64": "…",
/// "format"?: "pdf", "target_field"?: "due_date", "language"?: "en" }`.
///
/// It routes to the registered engine (`extraction::run_active`) and formats the
/// born-`Inferred` result as JSON — spans + measured confidence + provenance
/// (`inferred`) + taint (`untrusted`) + ceiling (`believe`) + the audit fields.
/// **On any failure it returns a typed refusal** (`success: false`, the error
/// slug), never an empty or invented string.
fn extraction_execute(transform: &str, argument: &str) -> ToolResult {
    use base64::Engine;
    let name = match transform {
        "pdf:text" => "PDFExtractor",
        "image:text" => "ImageTextExtractor",
        _ => "ImageAnalyzer",
    };
    let parsed: serde_json::Value = match serde_json::from_str(argument) {
        Ok(v) => v,
        Err(e) => return err(name, format!("invalid extraction request JSON: {e}")),
    };
    let bytes = match parsed.get("bytes_base64").and_then(|v| v.as_str()) {
        Some(b64) => match base64::engine::general_purpose::STANDARD.decode(b64) {
            Ok(b) => b,
            Err(e) => return err(name, format!("bytes_base64 is not valid base64: {e}")),
        },
        None => return err(name, "extraction request missing `bytes_base64`".into()),
    };
    let str_field = |k: &str| parsed.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());
    let hint = crate::extraction::ExtractionHint {
        format: str_field("format"),
        transform: Some(transform.to_string()),
        target_field: str_field("target_field"),
        language: str_field("language"),
    };
    match crate::extraction::run_active(&bytes, &hint, &crate::extraction::ExtractionBounds::default())
    {
        Ok(result) => {
            let spans: Vec<serde_json::Value> = result
                .spans
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "text": s.text,
                        "confidence": s.confidence,
                        "page": s.page,
                        "bbox": { "x": s.bbox.x, "y": s.bbox.y, "w": s.bbox.w, "h": s.bbox.h },
                    })
                })
                .collect();
            let out = serde_json::json!({
                "engine": result.engine,
                "engine_version": result.engine_version,
                "transform": transform,
                "provenance": "inferred", // the design decision — never `parsed`
                "taint": "untrusted", // the design decision
                "epistemic_ceiling": "believe", // the design decision — never `know`
                "mean_confidence": result.mean_confidence(),
                "page_count": result.page_count(),
                "spans": spans,
            });
            ToolResult { success: true, output: out.to_string(), tool_name: name.to_string() }
        }
        // Typed refusal — the whole point of v2.54.0 + the design decision: never fiction.
        Err(e) => err(name, format!("{}: {}", e.slug(), e)),
    }
}

fn err(tool: &str, msg: String) -> ToolResult {
    ToolResult {
        success: false,
        output: format!("DocumentRenderer: {msg}"),
        tool_name: tool.to_string(),
    }
}

/// v2.54.0 — read an ingested OOXML document. The argument is JSON:
/// `{ "bytes_base64": "…" }` (the host has already read + sandboxed the file),
/// OR `{ "path": "…", "roots": ["…"] }` (read through the v2.54.0 path sandbox).
/// Returns the born-Untrusted, Parsed text tree + per-part hashes.
// v2.81.0 — the OOXML surface lives behind the `documents` feature. The
// tool STAYS REGISTERED and refuses in writing: a capability that silently
// vanishes from a build is the v2.67.0 defect (an advertised primitive that is not
// there), and a runtime that answers "unknown tool" teaches the adopter nothing.
#[cfg(not(feature = "documents"))]
fn document_read_execute(_argument: &str) -> ToolResult {
    ToolResult {
        success: false,
        output: "DocumentReader: this build was compiled without the `documents` feature, so the OOXML engine is absent. Reinstall with: cargo install axon-lang --features documents"
            .to_string(),
        tool_name: "DocumentReader".to_string(),
    }
}

#[cfg(feature = "documents")]
fn document_read_execute(argument: &str) -> ToolResult {
    use base64::Engine;
    let name = "DocumentReader";
    let parsed: serde_json::Value = match serde_json::from_str(argument) {
        Ok(v) => v,
        Err(e) => return err_named(name, format!("invalid request JSON: {e}")),
    };
    // Resolve the bytes — either supplied directly, or read via the sandbox.
    let bytes: Vec<u8> = if let Some(b64) = parsed.get("bytes_base64").and_then(|v| v.as_str()) {
        match base64::engine::general_purpose::STANDARD.decode(b64) {
            Ok(b) => b,
            Err(e) => return err_named(name, format!("invalid base64: {e}")),
        }
    } else if let Some(path) = parsed.get("path").and_then(|v| v.as_str()) {
        let roots: Vec<std::path::PathBuf> = parsed
            .get("roots")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(std::path::PathBuf::from)).collect())
            .unwrap_or_default();
        let sandbox = crate::fs_sandbox::PathSandbox::new(roots);
        match sandbox.resolve(path) {
            Ok(resolved) => match std::fs::read(&resolved) {
                Ok(b) => b,
                Err(e) => return err_named(name, format!("read failed: {e}")),
            },
            Err(e) => return err_named(name, e.to_string()),
        }
    } else {
        return err_named(name, "request needs `bytes_base64` or `path`".into());
    };

    match crate::ooxml_read::read_ooxml(&bytes, &crate::ooxml_read::IngestBounds::default()) {
        Ok(doc) => {
            let text: Vec<serde_json::Value> = doc
                .text
                .iter()
                .map(|r| serde_json::json!({ "part": r.part, "text": r.text }))
                .collect();
            let out = serde_json::json!({
                "format": doc.format,
                // Born Untrusted, Parsed — the type system carries this.
                "taint": doc.taint.as_str(),
                "provenance": doc.provenance.as_str(),
                "epistemic_ceiling": doc.provenance.epistemic_ceiling(),
                "text": text,
                "full_text": doc.full_text(),
                "part_hashes": doc.part_hashes,
            });
            ToolResult { success: true, output: out.to_string(), tool_name: name.to_string() }
        }
        Err(e) => err_named(name, e.to_string()),
    }
}

/// v2.54.0 — surgical edit. Argument JSON:
/// `{ "bytes_base64": "…", "edits": [{ "kind": "replace_text", "part": "…",
/// "find": "…", "replace": "…" }] }`. Returns the new bytes + the per-part hash
/// manifest (the proven blast radius) + the inherited taint.
// v2.81.0 — the OOXML surface lives behind the `documents` feature. The
// tool STAYS REGISTERED and refuses in writing: a capability that silently
// vanishes from a build is the v2.67.0 defect (an advertised primitive that is not
// there), and a runtime that answers "unknown tool" teaches the adopter nothing.
#[cfg(not(feature = "documents"))]
fn document_edit_execute(_argument: &str) -> ToolResult {
    ToolResult {
        success: false,
        output: "DocumentEditor: this build was compiled without the `documents` feature, so the OOXML engine is absent. Reinstall with: cargo install axon-lang --features documents"
            .to_string(),
        tool_name: "DocumentEditor".to_string(),
    }
}

#[cfg(feature = "documents")]
fn document_edit_execute(argument: &str) -> ToolResult {
    use base64::Engine;
    let name = "DocumentEditor";
    let parsed: serde_json::Value = match serde_json::from_str(argument) {
        Ok(v) => v,
        Err(e) => return err_named(name, format!("invalid request JSON: {e}")),
    };
    let b64 = match parsed.get("bytes_base64").and_then(|v| v.as_str()) {
        Some(b) => b,
        None => return err_named(name, "request needs `bytes_base64`".into()),
    };
    let bytes = match base64::engine::general_purpose::STANDARD.decode(b64) {
        Ok(b) => b,
        Err(e) => return err_named(name, format!("invalid base64: {e}")),
    };
    let doc = match crate::ooxml_read::read_ooxml(&bytes, &crate::ooxml_read::IngestBounds::default()) {
        Ok(d) => d,
        Err(e) => return err_named(name, format!("read failed: {e}")),
    };
    // Parse the edits.
    let mut edits = Vec::new();
    for e in parsed.get("edits").and_then(|v| v.as_array()).cloned().unwrap_or_default() {
        let part = e.get("part").and_then(|v| v.as_str()).unwrap_or("").to_string();
        match e.get("kind").and_then(|v| v.as_str()) {
            Some("replace_text") => edits.push(crate::ooxml_edit::PartEdit::ReplaceText {
                part,
                find: e.get("find").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                replace: e.get("replace").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            }),
            Some("replace") => {
                let nb = e
                    .get("new_bytes_base64")
                    .and_then(|v| v.as_str())
                    .and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok())
                    .unwrap_or_default();
                edits.push(crate::ooxml_edit::PartEdit::Replace { part, new_bytes: nb });
            }
            other => return err_named(name, format!("unknown edit kind '{}'", other.unwrap_or("<none>"))),
        }
    }
    match crate::ooxml_edit::edit_document(&doc, &edits) {
        Ok(out) => {
            let manifest: Vec<serde_json::Value> = out
                .manifest
                .iter()
                .map(|m| serde_json::json!({
                    "part": m.part, "before": m.before_sha256, "after": m.after_sha256, "touched": m.touched,
                }))
                .collect();
            let result = serde_json::json!({
                "sha256": out.sha256_hex,
                // The edit inherits the input's taint — no laundering.
                "taint": out.taint.as_str(),
                "touched_parts": out.touched_parts(),
                "manifest": manifest,
                "bytes_base64": base64::engine::general_purpose::STANDARD.encode(&out.bytes),
            });
            ToolResult { success: true, output: result.to_string(), tool_name: name.to_string() }
        }
        Err(e) => err_named(name, e.to_string()),
    }
}

#[cfg_attr(not(feature = "postgres"), allow(dead_code))]
fn err_named(tool: &str, msg: String) -> ToolResult {
    ToolResult { success: false, output: format!("{tool}: {msg}"), tool_name: tool.to_string() }
}

// ── Calculator ──────────────────────────────────────────────────────────────

/// Safe arithmetic expression evaluator.
///
/// Supports: +, -, *, /, % (mod), ** (power), parentheses,
/// constants (pi, e), and functions (sqrt, abs, round, sin, cos, tan,
/// log, ln, ceil, floor, pow, min, max).
pub fn calculator_execute(expression: &str) -> ToolResult {
    let expr = expression.trim();
    if expr.is_empty() {
        return ToolResult {
            success: false,
            output: "Empty expression".to_string(),
            tool_name: "Calculator".to_string(),
        };
    }

    match eval_expr(expr) {
        Ok(val) => {
            // Format: remove trailing zeros for clean output
            let formatted = if val.fract() == 0.0 && val.abs() < 1e15 {
                format!("{}", val as i64)
            } else {
                format!("{}", val)
            };
            ToolResult {
                success: true,
                output: formatted,
                tool_name: "Calculator".to_string(),
            }
        }
        Err(e) => ToolResult {
            success: false,
            output: format!("Calculator error: {e}"),
            tool_name: "Calculator".to_string(),
        },
    }
}

// ── Calculator parser (recursive descent) ───────────────────────────────────

struct CalcParser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> CalcParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            pos: 0,
        }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.input.get(self.pos).copied()
    }

    fn consume(&mut self, expected: u8) -> bool {
        self.skip_ws();
        if self.pos < self.input.len() && self.input[self.pos] == expected {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// expr = term (('+' | '-') term)*
    fn parse_expr(&mut self) -> Result<f64, String> {
        let mut result = self.parse_term()?;
        loop {
            self.skip_ws();
            if self.consume(b'+') {
                result += self.parse_term()?;
            } else if self.consume(b'-') {
                result -= self.parse_term()?;
            } else {
                break;
            }
        }
        Ok(result)
    }

    /// term = power (('*' | '/' | '%') power)*
    fn parse_term(&mut self) -> Result<f64, String> {
        let mut result = self.parse_power()?;
        loop {
            self.skip_ws();
            if self.consume(b'*') {
                if self.consume(b'*') {
                    // ** is power — put it back and let power handle it
                    self.pos -= 2;
                    break;
                }
                result *= self.parse_power()?;
            } else if self.consume(b'/') {
                let divisor = self.parse_power()?;
                if divisor == 0.0 {
                    return Err("Division by zero".to_string());
                }
                result /= divisor;
            } else if self.consume(b'%') {
                let modulus = self.parse_power()?;
                if modulus == 0.0 {
                    return Err("Modulo by zero".to_string());
                }
                result %= modulus;
            } else {
                break;
            }
        }
        Ok(result)
    }

    /// power = unary ('**' unary)*
    fn parse_power(&mut self) -> Result<f64, String> {
        let base = self.parse_unary()?;
        self.skip_ws();
        if self.pos + 1 < self.input.len()
            && self.input[self.pos] == b'*'
            && self.input[self.pos + 1] == b'*'
        {
            self.pos += 2;
            let exp = self.parse_power()?; // right-associative
            Ok(base.powf(exp))
        } else {
            Ok(base)
        }
    }

    /// unary = '-' unary | '+' unary | atom
    fn parse_unary(&mut self) -> Result<f64, String> {
        self.skip_ws();
        if self.consume(b'-') {
            Ok(-self.parse_unary()?)
        } else if self.consume(b'+') {
            self.parse_unary()
        } else {
            self.parse_atom()
        }
    }

    /// atom = number | '(' expr ')' | function '(' args ')' | constant
    fn parse_atom(&mut self) -> Result<f64, String> {
        self.skip_ws();

        // Parenthesized expression
        if self.consume(b'(') {
            let val = self.parse_expr()?;
            if !self.consume(b')') {
                return Err("Missing closing parenthesis".to_string());
            }
            return Ok(val);
        }

        // Number
        if self.pos < self.input.len()
            && (self.input[self.pos].is_ascii_digit() || self.input[self.pos] == b'.')
        {
            return self.parse_number();
        }

        // Identifier (function or constant)
        if self.pos < self.input.len() && self.input[self.pos].is_ascii_alphabetic() {
            let name = self.parse_ident();
            return self.resolve_ident(&name);
        }

        Err(format!(
            "Unexpected character at position {}",
            self.pos
        ))
    }

    fn parse_number(&mut self) -> Result<f64, String> {
        let start = self.pos;
        while self.pos < self.input.len()
            && (self.input[self.pos].is_ascii_digit() || self.input[self.pos] == b'.')
        {
            self.pos += 1;
        }
        // Handle scientific notation: 1e10, 2.5e-3
        if self.pos < self.input.len()
            && (self.input[self.pos] == b'e' || self.input[self.pos] == b'E')
        {
            self.pos += 1;
            if self.pos < self.input.len()
                && (self.input[self.pos] == b'+' || self.input[self.pos] == b'-')
            {
                self.pos += 1;
            }
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        let s = std::str::from_utf8(&self.input[start..self.pos])
            .map_err(|_| "Invalid UTF-8 in number")?;
        s.parse::<f64>()
            .map_err(|_| format!("Invalid number: '{s}'"))
    }

    fn parse_ident(&mut self) -> String {
        let start = self.pos;
        while self.pos < self.input.len()
            && (self.input[self.pos].is_ascii_alphanumeric() || self.input[self.pos] == b'_')
        {
            self.pos += 1;
        }
        String::from_utf8_lossy(&self.input[start..self.pos]).to_string()
    }

    fn resolve_ident(&mut self, name: &str) -> Result<f64, String> {
        // Constants
        match name {
            "pi" | "PI" => return Ok(std::f64::consts::PI),
            "e" | "E" => return Ok(std::f64::consts::E),
            "tau" | "TAU" => return Ok(std::f64::consts::TAU),
            "inf" => return Ok(f64::INFINITY),
            _ => {}
        }

        // Functions
        self.skip_ws();
        if !self.consume(b'(') {
            return Err(format!("Unknown identifier: '{name}'"));
        }

        let args = self.parse_args()?;

        if !self.consume(b')') {
            return Err(format!("Missing ')' after {name}(...)"));
        }

        match (name, args.len()) {
            ("sqrt", 1) => Ok(args[0].sqrt()),
            ("abs", 1) => Ok(args[0].abs()),
            ("round", 1) => Ok(args[0].round()),
            ("ceil", 1) => Ok(args[0].ceil()),
            ("floor", 1) => Ok(args[0].floor()),
            ("sin", 1) => Ok(args[0].sin()),
            ("cos", 1) => Ok(args[0].cos()),
            ("tan", 1) => Ok(args[0].tan()),
            ("asin", 1) => Ok(args[0].asin()),
            ("acos", 1) => Ok(args[0].acos()),
            ("atan", 1) => Ok(args[0].atan()),
            ("log", 1) | ("log10", 1) => Ok(args[0].log10()),
            ("ln", 1) => Ok(args[0].ln()),
            ("log2", 1) => Ok(args[0].log2()),
            ("exp", 1) => Ok(args[0].exp()),
            ("pow", 2) => Ok(args[0].powf(args[1])),
            ("min", 2) => Ok(args[0].min(args[1])),
            ("max", 2) => Ok(args[0].max(args[1])),
            ("atan2", 2) => Ok(args[0].atan2(args[1])),
            _ => Err(format!("Unknown function: '{name}' with {} args", args.len())),
        }
    }

    fn parse_args(&mut self) -> Result<Vec<f64>, String> {
        let mut args = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b')') {
            return Ok(args);
        }
        args.push(self.parse_expr()?);
        while self.consume(b',') {
            args.push(self.parse_expr()?);
        }
        Ok(args)
    }
}

fn eval_expr(expr: &str) -> Result<f64, String> {
    let mut parser = CalcParser::new(expr);
    let result = parser.parse_expr()?;
    parser.skip_ws();
    if parser.pos < parser.input.len() {
        return Err(format!(
            "Unexpected trailing characters at position {}",
            parser.pos
        ));
    }
    if result.is_nan() {
        return Err("Result is NaN".to_string());
    }
    Ok(result)
}

// ── DateTimeTool ────────────────────────────────────────────────────────────

/// Current date/time queries using system time (UTC).
///
/// Supported queries: now, today, timestamp, year, month, day, weekday, iso,
/// hour, minute, second, date, time.
pub fn datetime_execute(query: &str) -> ToolResult {
    let query = query.trim().to_lowercase();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();

    let secs = now.as_secs();
    let (year, month, day, hour, min, sec, weekday) = unix_to_utc(secs);

    let output = match query.as_str() {
        "now" | "iso" | "datetime" => format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            year, month, day, hour, min, sec
        ),
        "today" | "date" => format!("{:04}-{:02}-{:02}", year, month, day),
        "time" => format!("{:02}:{:02}:{:02}Z", hour, min, sec),
        "timestamp" | "unix" | "epoch" => format!("{}", secs),
        "year" => format!("{}", year),
        "month" => format!("{}", month),
        "day" => format!("{}", day),
        "hour" => format!("{}", hour),
        "minute" => format!("{}", min),
        "second" => format!("{}", sec),
        "weekday" => weekday_name(weekday).to_string(),
        _ => format!(
            "Unknown query '{}'. Supported: now, today, timestamp, year, month, day, weekday, iso, time, hour, minute, second",
            query
        ),
    };

    ToolResult {
        success: true,
        output,
        tool_name: "DateTimeTool".to_string(),
    }
}

/// Convert UNIX timestamp to (year, month, day, hour, min, sec, weekday).
/// weekday: 0=Sunday, 1=Monday, ..., 6=Saturday.
fn unix_to_utc(secs: u64) -> (i32, u32, u32, u32, u32, u32, u32) {
    let days = (secs / 86400) as i64;
    let time_of_day = secs % 86400;

    let hour = (time_of_day / 3600) as u32;
    let min = ((time_of_day % 3600) / 60) as u32;
    let sec = (time_of_day % 60) as u32;

    // Weekday: Jan 1, 1970 was Thursday (4)
    let weekday = ((days + 4) % 7) as u32;

    // Civil date from days since epoch (algorithm from Howard Hinnant)
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64 + era * 400) as i32;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    (year, m, d, hour, min, sec, weekday)
}

fn weekday_name(weekday: u32) -> &'static str {
    match weekday {
        0 => "Sunday",
        1 => "Monday",
        2 => "Tuesday",
        3 => "Wednesday",
        4 => "Thursday",
        5 => "Friday",
        6 => "Saturday",
        _ => "Unknown",
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Calculator tests

    #[test]
    fn calc_basic_arithmetic() {
        assert_eq!(eval_expr("2 + 3").unwrap(), 5.0);
        assert_eq!(eval_expr("10 - 4").unwrap(), 6.0);
        assert_eq!(eval_expr("3 * 7").unwrap(), 21.0);
        assert_eq!(eval_expr("20 / 4").unwrap(), 5.0);
    }

    #[test]
    fn calc_operator_precedence() {
        assert_eq!(eval_expr("2 + 3 * 4").unwrap(), 14.0);
        assert_eq!(eval_expr("(2 + 3) * 4").unwrap(), 20.0);
    }

    #[test]
    fn calc_power() {
        assert_eq!(eval_expr("2 ** 10").unwrap(), 1024.0);
        assert_eq!(eval_expr("3 ** 2").unwrap(), 9.0);
    }

    #[test]
    fn calc_modulo() {
        assert_eq!(eval_expr("17 % 5").unwrap(), 2.0);
    }

    #[test]
    fn calc_unary_minus() {
        assert_eq!(eval_expr("-5").unwrap(), -5.0);
        assert_eq!(eval_expr("-3 + 7").unwrap(), 4.0);
        assert_eq!(eval_expr("-(2 + 3)").unwrap(), -5.0);
    }

    #[test]
    fn calc_constants() {
        assert!((eval_expr("pi").unwrap() - std::f64::consts::PI).abs() < 1e-10);
        assert!((eval_expr("e").unwrap() - std::f64::consts::E).abs() < 1e-10);
    }

    #[test]
    fn calc_functions() {
        assert_eq!(eval_expr("sqrt(16)").unwrap(), 4.0);
        assert_eq!(eval_expr("abs(-5)").unwrap(), 5.0);
        assert_eq!(eval_expr("round(3.7)").unwrap(), 4.0);
        assert_eq!(eval_expr("ceil(3.2)").unwrap(), 4.0);
        assert_eq!(eval_expr("floor(3.8)").unwrap(), 3.0);
        assert_eq!(eval_expr("pow(2, 8)").unwrap(), 256.0);
        assert_eq!(eval_expr("min(3, 7)").unwrap(), 3.0);
        assert_eq!(eval_expr("max(3, 7)").unwrap(), 7.0);
    }

    #[test]
    fn calc_trig() {
        assert!((eval_expr("sin(0)").unwrap()).abs() < 1e-10);
        assert!((eval_expr("cos(0)").unwrap() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn calc_logarithm() {
        assert!((eval_expr("log(100)").unwrap() - 2.0).abs() < 1e-10);
        assert!((eval_expr("ln(e)").unwrap() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn calc_nested() {
        assert_eq!(eval_expr("sqrt(pow(3, 2) + pow(4, 2))").unwrap(), 5.0);
    }

    #[test]
    fn calc_scientific_notation() {
        assert_eq!(eval_expr("1e3").unwrap(), 1000.0);
        assert_eq!(eval_expr("2.5e2").unwrap(), 250.0);
    }

    #[test]
    fn calc_division_by_zero() {
        assert!(eval_expr("1 / 0").is_err());
    }

    #[test]
    fn calc_empty_expression() {
        let r = calculator_execute("");
        assert!(!r.success);
    }

    #[test]
    fn calc_invalid_expression() {
        assert!(eval_expr("2 +").is_err());
    }

    #[test]
    fn calc_integer_output() {
        let r = calculator_execute("2 + 3");
        assert!(r.success);
        assert_eq!(r.output, "5");
    }

    #[test]
    fn calc_float_output() {
        let r = calculator_execute("1 / 3");
        assert!(r.success);
        assert!(r.output.starts_with("0.333"));
    }

    // DateTimeTool tests

    #[test]
    fn datetime_now_iso_format() {
        let r = datetime_execute("now");
        assert!(r.success);
        assert!(r.output.contains('T'));
        assert!(r.output.ends_with('Z'));
    }

    #[test]
    fn datetime_today() {
        let r = datetime_execute("today");
        assert!(r.success);
        assert_eq!(r.output.len(), 10); // YYYY-MM-DD
        assert!(r.output.contains('-'));
    }

    #[test]
    fn datetime_timestamp() {
        let r = datetime_execute("timestamp");
        assert!(r.success);
        let ts: u64 = r.output.parse().expect("should be a number");
        assert!(ts > 1700000000); // After ~2023
    }

    #[test]
    fn datetime_year() {
        let r = datetime_execute("year");
        assert!(r.success);
        let y: i32 = r.output.parse().expect("should be a number");
        assert!(y >= 2024);
    }

    #[test]
    fn datetime_weekday() {
        let r = datetime_execute("weekday");
        assert!(r.success);
        let valid = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
        assert!(valid.contains(&r.output.as_str()));
    }

    #[test]
    fn datetime_unknown_query() {
        let r = datetime_execute("foobar");
        assert!(r.success);
        assert!(r.output.contains("Unknown query"));
    }

    // Dispatch tests

    #[test]
    fn dispatch_calculator() {
        let r = dispatch("Calculator", "2 + 2");
        assert!(r.is_some());
        let r = r.unwrap();
        assert!(r.success);
        assert_eq!(r.output, "4");
    }

    #[test]
    fn dispatch_datetime() {
        let r = dispatch("DateTimeTool", "now");
        assert!(r.is_some());
        assert!(r.unwrap().success);
    }

    #[test]
    fn dispatch_unknown_tool() {
        assert!(dispatch("WebSearch", "query").is_none());
    }
}
