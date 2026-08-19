//! MCP prompts — parameterized prompt templates the host surfaces to
//! the human user as named recipes (slash-commands, chat-menu entries,
//! prompt pickers).
//!
//! Where **tools** are calls the agent issues and **resources** are
//! documents the agent reads, **prompts** are *kickoff messages* the
//! user picks. The host renders the prompt with the user-supplied
//! arguments and inserts the resulting message into the conversation;
//! the agent then drives the rest of the work using `tools/*` and
//! `resources/*`.
//!
//! Phase 5 ships three:
//!
//! - `flow_design`    — design an AXON flow from a natural-language intent
//! - `shield_design`  — design an AXON shield for a given threat/purpose
//! - `session_design` — design a v2.3.0 duality-correct session + socket
//!
//! Wire layer: `prompts/list` returns the catalogue (name + description
//! + arguments schema); `prompts/get` renders one prompt with the
//! supplied arguments and returns `{ description, messages: [...] }`.

use std::collections::HashMap;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::knowledge::Catalog;
use crate::server::JsonRpcError;
use crate::telemetry::Telemetry;

/// Build the `prompts/list` payload. Each entry advertises the
/// prompt's name, summary, and arguments schema so the host can
/// render a form-style picker before calling `prompts/get`.
///
/// Stable iteration order — `Catalog::prompts()` walks the underlying
/// `BTreeMap` so two runs of `prompts/list` over the same corpus
/// emit byte-identical responses.
pub fn list(catalog: &Arc<Catalog>) -> Vec<Value> {
    catalog
        .prompts()
        .map(|p| {
            json!({
                "name": p.name,
                "description": p.summary,
                "arguments": p.arguments,
            })
        })
        .collect()
}

/// Dispatch a `prompts/get` request. Params shape (per MCP spec):
/// `{ "name": "...", "arguments": { "<arg>": "<value>", ... } }`.
///
/// v1.3.0 — every dispatch records a `prompt_get` event with the
/// prompt name + a `missing_required` boolean (no argument values are
/// recorded — they're caller-supplied free-form text that may carry
/// PII).
pub fn dispatch_get(
    params: Value,
    catalog: &Arc<Catalog>,
    telemetry: &Arc<Telemetry>,
) -> Result<Value, JsonRpcError> {
    let req: GetParams = serde_json::from_value(params)
        .map_err(|e| JsonRpcError::invalid_params(format!("prompts/get params: {e}")))?;

    let prompt = catalog.prompt(&req.name).ok_or_else(|| JsonRpcError {
        code: -32602,
        message: format!(
            "unknown prompt `{}` — call prompts/list to see the available names",
            req.name
        ),
        data: None,
    })?;

    // Every required argument must be present + non-empty. Missing
    // required args surface a structured invalid_params so the host
    // can re-prompt the user; missing optionals render as
    // `(unspecified)` in-text (sentinel chosen for human readability).
    let args = req.arguments.unwrap_or_default();
    for declared in &prompt.arguments {
        if declared.required {
            let v = args.get(&declared.name);
            let is_empty = v.map(|s| s.trim().is_empty()).unwrap_or(true);
            if is_empty {
                telemetry.record_prompt_get(&req.name, /* missing_required */ true);
                return Err(JsonRpcError::invalid_params(format!(
                    "prompts/get `{}`: required argument `{}` is missing or empty",
                    req.name, declared.name
                )));
            }
        }
    }

    telemetry.record_prompt_get(&req.name, /* missing_required */ false);
    let rendered = render(&prompt.body, &args);

    // Per MCP spec, a `prompts/get` reply carries an optional
    // `description` plus a `messages` array of role-tagged content
    // blocks. We surface one `user` message with the rendered body —
    // hosts will inject that as the first turn of the conversation.
    Ok(json!({
        "description": prompt.title,
        "messages": [
            {
                "role": "user",
                "content": { "type": "text", "text": rendered }
            }
        ]
    }))
}

/// Substitute `{{name}}` placeholders in `body` with the supplied
/// argument values. Unsupplied placeholders render as the literal
/// `(unspecified)` so the rendered text is always grammatical (a
/// raw `{{name}}` left in the output would confuse the agent).
///
/// The renderer is intentionally simple — no nested expressions, no
/// conditionals, no escape sequences. Anyone who needs richer
/// templating can author a richer prompt or post-process the
/// rendered output; we are not building a templating engine.
fn render(body: &str, args: &HashMap<String, String>) -> String {
    // We walk the body byte-wise but match `{{...}}` against char
    // boundaries to stay UTF-8 safe. The body is bounded in size
    // (a few KB at most), so an O(n·m) substitution is fine.
    let mut out = String::with_capacity(body.len() + 64);
    let mut rest = body;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        let Some(end) = after_open.find("}}") else {
            // No closing brace pair — preserve the literal "{{" and
            // continue past it so the body parses idempotently.
            out.push_str(&rest[start..]);
            return out;
        };
        let name = after_open[..end].trim();
        let value = args
            .get(name)
            .map(|s| s.as_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("(unspecified)");
        out.push_str(value);
        rest = &after_open[end + 2..];
    }
    out.push_str(rest);
    out
}

#[derive(Debug, Deserialize)]
struct GetParams {
    name: String,
    #[serde(default)]
    arguments: Option<HashMap<String, String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn embedded() -> Arc<Catalog> {
        Arc::new(Catalog::load_embedded().expect("embedded corpus must load"))
    }

    /// Throwaway telemetry registry — JSONL sink disabled, deployment
    /// ID empty. Cheap per test.
    fn tel() -> Arc<Telemetry> {
        Arc::new(Telemetry::new(crate::telemetry::TelemetryConfig {
            jsonl_sink: None,
            deployment_id: "".into(),
            max_samples: 1000,
        }))
    }

    // ── render() ─────────────────────────────────────────────────────

    #[test]
    fn render_substitutes_known_arg_placeholders() {
        let mut args = HashMap::new();
        args.insert("intent".to_string(), "summarise a patient record".to_string());
        let out = render("You will: {{intent}}.", &args);
        assert_eq!(out, "You will: summarise a patient record.");
    }

    #[test]
    fn render_substitutes_unspecified_when_arg_missing() {
        let args = HashMap::new();
        let out = render("Domain: {{domain}}.", &args);
        assert_eq!(out, "Domain: (unspecified).");
    }

    #[test]
    fn render_substitutes_unspecified_when_arg_is_empty_string() {
        // An optional argument supplied as an empty string is treated
        // identically to one omitted — the user did not provide a
        // value, so the rendered text says so.
        let mut args = HashMap::new();
        args.insert("streaming".to_string(), "".to_string());
        let out = render("Streaming: {{streaming}}.", &args);
        assert_eq!(out, "Streaming: (unspecified).");
    }

    #[test]
    fn render_tolerates_unbalanced_braces() {
        // A `{{` without a closing `}}` is preserved verbatim — the
        // renderer must not panic on malformed templates.
        let args = HashMap::new();
        let out = render("trailing {{ open", &args);
        assert_eq!(out, "trailing {{ open");
    }

    #[test]
    fn render_handles_multiple_occurrences_of_same_arg() {
        let mut args = HashMap::new();
        args.insert("x".to_string(), "yes".to_string());
        let out = render("a={{x}} b={{x}} c={{x}}", &args);
        assert_eq!(out, "a=yes b=yes c=yes");
    }

    #[test]
    fn render_trims_whitespace_inside_placeholder_braces() {
        let mut args = HashMap::new();
        args.insert("intent".to_string(), "X".to_string());
        let out = render("You will: {{  intent  }}.", &args);
        assert_eq!(out, "You will: X.");
    }

    // ── list ─────────────────────────────────────────────────────────

    #[test]
    fn list_emits_one_entry_per_embedded_prompt() {
        let cat = embedded();
        let entries = list(&cat);
        // Phase 5 ships exactly 3 prompts — the count is a regression
        // signal more than a hard cap; future phases can grow the set.
        assert!(entries.len() >= 3, "expected ≥ 3 prompts, saw {}", entries.len());
        let names: Vec<&str> = entries
            .iter()
            .map(|e| e["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"flow_design"));
        assert!(names.contains(&"shield_design"));
        assert!(names.contains(&"session_design"));
        // Every entry advertises the MCP-spec triplet.
        for e in &entries {
            assert!(e["name"].is_string());
            assert!(e["description"].is_string());
            assert!(e["arguments"].is_array());
        }
    }

    // ── dispatch_get ─────────────────────────────────────────────────

    #[test]
    fn dispatch_get_renders_flow_design_with_arguments() {
        let cat = embedded();
        let v = dispatch_get(
            json!({
                "name": "flow_design",
                "arguments": {
                    "intent": "summarise a patient record",
                    "domain": "healthcare",
                    "streaming": "no",
                    "compliance": "HIPAA, GDPR"
                }
            }),
            &cat, &tel(),
        )
        .unwrap();
        // Description is the prompt's title.
        assert_eq!(v["description"], "Design an AXON flow");
        // Messages array with one user-role text block.
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        let text = msgs[0]["content"]["text"].as_str().unwrap();
        // Every supplied argument is substituted into the rendered body.
        assert!(text.contains("summarise a patient record"));
        assert!(text.contains("**healthcare**"));
        assert!(text.contains("**no**"));
        assert!(text.contains("**HIPAA, GDPR**"));
        // The body must NOT carry an unrendered `{{intent}}` placeholder.
        assert!(!text.contains("{{intent}}"));
    }

    #[test]
    fn dispatch_get_renders_unspecified_for_missing_optional_arguments() {
        let cat = embedded();
        let v = dispatch_get(
            json!({
                "name": "flow_design",
                "arguments": { "intent": "ship a thing" }
            }),
            &cat, &tel(),
        )
        .unwrap();
        let text = v["messages"][0]["content"]["text"].as_str().unwrap();
        assert!(text.contains("ship a thing"));
        // `domain`, `streaming`, `compliance` are optional — the
        // renderer fills them as `(unspecified)`.
        assert!(text.contains("**(unspecified)**"));
    }

    #[test]
    fn dispatch_get_rejects_missing_required_argument() {
        let cat = embedded();
        let err = dispatch_get(
            json!({ "name": "flow_design", "arguments": {} }),
            &cat, &tel(),
        )
        .expect_err("missing required `intent` must reject");
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("required argument `intent`"));
    }

    #[test]
    fn dispatch_get_rejects_empty_required_argument() {
        let cat = embedded();
        let err = dispatch_get(
            json!({ "name": "shield_design", "arguments": { "purpose": "   " } }),
            &cat, &tel(),
        )
        .expect_err("whitespace-only required argument must be treated as missing");
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("required argument `purpose`"));
    }

    #[test]
    fn dispatch_get_rejects_unknown_prompt_name() {
        let cat = embedded();
        let err = dispatch_get(
            json!({ "name": "does_not_exist", "arguments": {} }),
            &cat, &tel(),
        )
        .expect_err("unknown prompt must surface a structured error");
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("unknown prompt"));
        assert!(err.message.contains("prompts/list"));
    }

    #[test]
    fn dispatch_get_accepts_session_design_with_full_argument_set() {
        let cat = embedded();
        let v = dispatch_get(
            json!({
                "name": "session_design",
                "arguments": {
                    "intent": "turn-taking chat with cancellation",
                    "parties": "2",
                    "backpressure": "8",
                    "reconnect": "yes"
                }
            }),
            &cat, &tel(),
        )
        .unwrap();
        let text = v["messages"][0]["content"]["text"].as_str().unwrap();
        assert!(text.contains("turn-taking chat with cancellation"));
        assert!(text.contains("Parties: **2**"));
        assert!(text.contains("Credit window: **8**"));
        assert!(text.contains("Reconnect-on-disconnect: **yes**"));
    }
}
