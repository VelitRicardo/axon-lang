//! MCP tools the connected agent can invoke.
//!
//! Each tool is a `(name, description, inputSchema)` triple advertised
//! via `tools/list`, plus a handler dispatched via `tools/call`. The
//! `inputSchema` is JSON Schema (draft-07) — the agent uses it to
//! shape its calls, so it must be honest about every required field.
//!
//! Phase 0 ships **two** tools:
//!
//! - `axon.primitives` — list every primitive (optionally filtered by
//!   category). Lets the agent ground itself in the catalogue before
//!   composing.
//! - `axon.primitive_doc` — fetch the full markdown reference for one
//!   primitive (grammar, top-level status, since-version, prose body).
//!
//! Subsequent phases extend this set with `axon.check`, `axon.parse`,
//! `axon.examples`, `axon.compose`, `axon.validate_pattern`.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};

use std::time::Instant;

use crate::compiler_pipeline;
use crate::compose;
use crate::knowledge::{Catalog, Category, ExampleTopic};
use crate::server::JsonRpcError;
use crate::telemetry::Telemetry;

/// Build the `tools/list` payload. Sorted by name for a deterministic
/// wire — the agent should not see the order shift across runs.
pub fn list() -> Vec<Value> {
    vec![
        json!({
            "name": "axon.primitives",
            "description": "List every AXON primitive known to this server. \
                Optionally filter by category. Use this FIRST when you need \
                to know what's available before composing a program. \
                Returns a sorted array of {name, summary, category, top_level, since}.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "enum": [
                            "cognition", "cognitive_io", "data_plane",
                            "session_types", "wire", "operators"
                        ],
                        "description": "Limit to one primitive family. Omit to list all."
                    }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "axon.primitive_doc",
            "description": "Fetch the full reference for one AXON primitive by canonical \
                name. Returns its grammar fragment, top-level status, the cycle that \
                introduced it, and the complete markdown body (semantic constraints, \
                examples, what-it-is-not, see-also). ALWAYS call this before generating \
                a program that uses the primitive — the grammar is precise and the \
                diagnostics are strict.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Canonical primitive name (e.g. \"persona\", \
                            \"flow\", \"socket\", \"axonendpoint\")."
                    }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "axon.check",
            "description": "Validate AXON source code. Runs the same lex → parse → \
                type-check pipeline the `axon check` CLI uses, and returns structured \
                diagnostics (severity + stage + message + line/column). Use this AFTER \
                generating a program, BEFORE presenting it to the user as working. \
                Never claim a program is correct until axon.check returns ok=true. The \
                pipeline stops at the first failing stage: a lex error means parse + \
                type-check did not run.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source": {
                        "type": "string",
                        "description": "The complete AXON source text to validate. Pass \
                            the whole program (top-level declarations only — fragments \
                            are usually rejected)."
                    },
                    "filename": {
                        "type": "string",
                        "description": "Optional virtual filename for the diagnostics. \
                            Defaults to \"<axon.check input>\"."
                    }
                },
                "required": ["source"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "axon.parse",
            "description": "Parse AXON source to its Intermediate Representation (IR). \
                Returns the same JSON shape `axon parse` would print: a `Program` node \
                whose children are every declared primitive (`flows`, `personas`, \
                `axonendpoints`, `axonstores`, `sockets`, `sessions`, …). Use this when \
                you need to REASON ABOUT what you just wrote — e.g. to confirm the \
                program has the right shape before refining it. On failure returns the \
                same diagnostic shape as axon.check.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source": {
                        "type": "string",
                        "description": "The complete AXON source text to parse."
                    },
                    "filename": {
                        "type": "string",
                        "description": "Optional virtual filename. Defaults to \
                            \"<axon.parse input>\"."
                    }
                },
                "required": ["source"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "axon.examples",
            "description": "Return focused, idiomatic AXON example programs from the curated corpus. \
                Where `axon.compose` returns a full-app scaffold organised by DOMAIN \
                (healthcare, banking, …), `axon.examples` returns minimal complete programs \
                (~20–60 LOC) organised by IDEA — `weave` braiding, session-type duality, \
                stream-with-backpressure, idempotent endpoints, etc. EVERY example is drift-\
                gated through `axon-frontend` so what you receive is guaranteed to compile. \
                USE THIS when you need to see how to use ONE primitive correctly before \
                composing a larger program — `axon.examples(primitive: \"weave\")` returns \
                every example that exercises `weave`. \
                Three filter modes (combine freely): \
                `name:` pins one specific example by slug; `topic:` filters by the closed \
                10-entry topic catalog; `primitive:` filters by primitive name. Omit all \
                three to get a listing of every example. \
                Returns `{count, examples: [{name, title, summary, topic, primitives, \
                source?}]}` — `source` is only included on single-example resolution.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Exact example slug (e.g. \"weave_braid\", \
                            \"session_chat_duality\"). When set, the response contains \
                            ONLY that one example, with its full `.axon` source."
                    },
                    "topic": {
                        "type": "string",
                        "enum": [
                            "composition", "session_types", "shields", "effects",
                            "streaming", "data", "agents", "endpoints", "memory",
                            "validation"
                        ],
                        "description": "Filter by the closed topic catalog (10 entries). \
                            Returns every example carrying this topic."
                    },
                    "primitive": {
                        "type": "string",
                        "description": "Filter by primitive name — returns every example \
                            that exercises this primitive idiomatically (free-form so the \
                            agent can ask about any of the 45 primitives in the registry)."
                    }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "axon.compose",
            "description": "Generate a typed AXON scaffold from a natural-language intent. \
                Classifies the intent into one of 8 closed domains (generic, healthcare, \
                banking, government, legal, chat, retrieval, multi_agent), fetches the \
                hand-authored template for that domain, re-validates it through the same \
                `axon-frontend` pipeline `axon.check` uses, and returns: \
                `{scaffold, domain, alternatives, primitives_used, compliance_applied, \
                next_steps, axon_check_verdict}`. \
                USE THIS when a user describes WHAT they want in plain language; the result \
                is a guaranteed-compile starting point you can iterate on. The classifier \
                is keyword-based and explainable — `alternatives` carries the scoreboard. \
                Override the classifier with `domain:` when you already know the domain.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "intent": {
                        "type": "string",
                        "description": "Natural-language description of what the user wants \
                            to build (\"a healthcare flow that handles PHI\", \"a streaming \
                            chat assistant\", \"a banking endpoint for loan decisions\")."
                    },
                    "domain": {
                        "type": "string",
                        "enum": [
                            "generic", "healthcare", "banking", "government", "legal",
                            "chat", "retrieval", "multi_agent",
                            "legaltech", "fintech", "pharmatech", "medic_research",
                            "chat_research", "chat_tools", "chat_skills", "whatsapp",
                            "voice", "dev", "sales_consultive", "sales_widget",
                            "workflow_automation", "business_intelligence",
                            "corporate_integration", "self_learning",
                            "document_analysis", "ticket_triage",
                            "content_moderation", "knowledge_extraction",
                            "compliance_monitoring", "recruitment",
                            "education", "financial_advisor", "data_pipeline"
                        ],
                        "description": "Optional explicit domain override. Skips the \
                            classifier — use when you already know which scaffold you want. \
                            v1.2.0 ships 33 closed domains: verticals (v1.2.0), agent \
                            patterns (v1.2.0), application patterns (v1.2.0). See \
                            `axon://logic/flow_composition` for picking the right one."
                    }
                },
                "required": ["intent"],
                "additionalProperties": false
            }
        }),
    ]
}

/// Dispatch a `tools/call` request. `params` is the raw `params` field
/// of the JSON-RPC envelope: `{ "name": "...", "arguments": { ... } }`.
///
/// v1.3.0 — every dispatched call is wall-clock-timed and recorded
/// through `telemetry`. The recorded fields are privacy-clean: tool
/// name + duration + `is_error` boolean. Arguments + error messages
/// are NEVER passed to the recorder (see `telemetry.rs` Privacy).
pub async fn dispatch_call(
    params: Value,
    catalog: &Arc<Catalog>,
    telemetry: &Arc<Telemetry>,
) -> Result<Value, JsonRpcError> {
    let call: ToolCall = serde_json::from_value(params)
        .map_err(|e| JsonRpcError::invalid_params(format!("tools/call params: {e}")))?;
    let started = Instant::now();
    // v1.3.0 — for `axon.compose` + `axon.check` + `axon.parse` we
    // ALSO record the structured outcome below (per-domain, per-stage)
    // through the dedicated `record_compose` / `record_check`
    // entrypoints. The top-level `record_tool_call` runs in EVERY
    // branch so the wire-level aggregates stay complete.
    let result = match call.name.as_str() {
        "axon.primitives" => primitives(call.arguments, catalog),
        "axon.primitive_doc" => primitive_doc(call.arguments, catalog),
        "axon.check" => check(call.arguments, telemetry),
        "axon.parse" => parse(call.arguments, telemetry),
        "axon.compose" => compose_tool(call.arguments, catalog, telemetry),
        "axon.examples" => examples(call.arguments, catalog, telemetry),
        other => Err(JsonRpcError {
            code: -32601,
            message: format!("unknown tool: `{other}`"),
            data: None,
        }),
    };
    let duration = started.elapsed();
    // `is_error` reflects the *envelope* outcome: a JSON-RPC error
    // (the Err arm) OR an `isError: true` flip inside a successful
    // envelope (the agent's reflex hook). Both are operationally
    // failed calls; we count them as such.
    let is_error = match &result {
        Err(_) => true,
        Ok(v) => v["isError"].as_bool().unwrap_or(false),
    };
    telemetry.record_tool_call(&call.name, duration, is_error);
    result
}

#[derive(Debug, Deserialize)]
struct ToolCall {
    name: String,
    #[serde(default)]
    arguments: Value,
}

// ─── axon.primitives ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct PrimitivesArgs {
    #[serde(default)]
    category: Option<String>,
}

fn primitives(args: Value, catalog: &Arc<Catalog>) -> Result<Value, JsonRpcError> {
    let args: PrimitivesArgs = if args.is_null() {
        PrimitivesArgs::default()
    } else {
        serde_json::from_value(args)
            .map_err(|e| JsonRpcError::invalid_params(format!("axon.primitives: {e}")))?
    };
    let filter = args.category.as_deref().map(parse_category).transpose()?;
    let entries: Vec<Value> = catalog
        .primitives()
        .filter(|p| filter.map(|f| p.category == f).unwrap_or(true))
        .map(|p| {
            json!({
                "name": p.name,
                "summary": p.summary,
                "category": p.category.as_str(),
                "top_level": p.top_level,
                "since": p.since,
                // The listing carries it too: an agent that filters this
                // list never opens the doc, and a name in a catalogue reads
                // as an endorsement unless something says otherwise.
                "runtime_status": p.reality.map(|r| r.slug()),
                "deliverable": p.reality.map(|r| r.is_deliverable()),
            })
        })
        .collect();
    Ok(mcp_text_result(&serde_json::to_string_pretty(&json!({
        "count": entries.len(),
        "primitives": entries,
    })).unwrap()))
}

fn parse_category(s: &str) -> Result<Category, JsonRpcError> {
    match s {
        "cognition" => Ok(Category::Cognition),
        "cognitive_io" => Ok(Category::CognitiveIo),
        "data_plane" => Ok(Category::DataPlane),
        "session_types" => Ok(Category::SessionTypes),
        "wire" => Ok(Category::Wire),
        "operators" => Ok(Category::Operators),
        other => Err(JsonRpcError::invalid_params(format!(
            "unknown category `{other}` — valid: cognition, cognitive_io, \
             data_plane, session_types, wire, operators"
        ))),
    }
}

// ─── axon.primitive_doc ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct PrimitiveDocArgs {
    name: String,
}

fn primitive_doc(args: Value, catalog: &Arc<Catalog>) -> Result<Value, JsonRpcError> {
    let args: PrimitiveDocArgs = serde_json::from_value(args)
        .map_err(|e| JsonRpcError::invalid_params(format!("axon.primitive_doc: {e}")))?;
    let prim = catalog.primitive(&args.name).ok_or_else(|| JsonRpcError {
        code: -32602,
        message: format!(
            "unknown primitive `{}` — call axon.primitives to see the catalogue",
            args.name
        ),
        data: None,
    })?;
    // We return both a structured `metadata` block (for the agent's
    // programmatic use) and a `text` block (the markdown body — which
    // is what the agent should actually quote / read).
    // v0.53.0 — the runtime reality travels WITH the reference, and the
    // warning goes FIRST. An agent reading a complete, confident page about a
    // primitive nothing calls will write it into an adopter's program; a
    // caveat at the bottom is a caveat it may never reach. Read from the
    // compiler ledger, so a reclassification arrives here with nothing to
    // update.
    let warning = prim.reality.map(|r| r.warning()).unwrap_or(
        "⚠️ NO LEDGER ROW — this primitive is documented and the compiler's advertised \
         table says nothing about what its runtime does. Treat the text below as \
         unverified.",
    );
    let body = if warning.is_empty() {
        prim.body.clone()
    } else {
        format!("> {warning}\n\n{}", prim.body)
    };
    let payload = json!({
        "name": prim.name,
        "summary": prim.summary,
        "category": prim.category.as_str(),
        "top_level": prim.top_level,
        "grammar": prim.grammar,
        "since": prim.since,
        "runtime_status": prim.reality.map(|r| r.slug()),
        "deliverable": prim.reality.map(|r| r.is_deliverable()),
        "body_markdown": body,
    });
    Ok(json!({
        "content": [
            { "type": "text", "text": serde_json::to_string_pretty(&payload).unwrap() }
        ],
        "isError": false,
    }))
}

/// Wrap a string result in the canonical MCP `tools/call` content
/// shape: `{ content: [{ type: "text", text: ... }], isError: false }`.
fn mcp_text_result(text: &str) -> Value {
    json!({
        "content": [
            { "type": "text", "text": text }
        ],
        "isError": false,
    })
}

// ─── axon.check ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CheckArgs {
    source: String,
    #[serde(default)]
    filename: Option<String>,
}

fn check(args: Value, telemetry: &Arc<Telemetry>) -> Result<Value, JsonRpcError> {
    let args: CheckArgs = serde_json::from_value(args)
        .map_err(|e| JsonRpcError::invalid_params(format!("axon.check: {e}")))?;
    let filename = args.filename.as_deref().unwrap_or("<axon.check input>");
    let outcome = compiler_pipeline::run(&args.source, filename);
    let payload = compiler_pipeline::outcome_to_check_payload(&outcome);
    let is_error = !payload["ok"].as_bool().unwrap_or(true);
    // v1.3.0 — record the per-stage outcome. The `stage` value is
    // the closed `Stage::as_str()` slug; the source itself is NEVER
    // forwarded to the recorder (privacy invariant #1).
    let stage_slug = payload["stage"].as_str().unwrap_or("type_check");
    telemetry.record_check(stage_slug, is_error);
    Ok(json!({
        "content": [
            { "type": "text", "text": serde_json::to_string_pretty(&payload).unwrap() }
        ],
        "isError": is_error,
    }))
}

// ─── axon.parse ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ParseArgs {
    source: String,
    #[serde(default)]
    filename: Option<String>,
}

fn parse(args: Value, telemetry: &Arc<Telemetry>) -> Result<Value, JsonRpcError> {
    let args: ParseArgs = serde_json::from_value(args)
        .map_err(|e| JsonRpcError::invalid_params(format!("axon.parse: {e}")))?;
    let filename = args.filename.as_deref().unwrap_or("<axon.parse input>");
    let outcome = compiler_pipeline::run(&args.source, filename);
    let payload = compiler_pipeline::outcome_to_parse_payload(outcome);
    let is_error = !payload["ok"].as_bool().unwrap_or(true);
    // v1.3.0 — same per-stage recording surface as `axon.check`. The
    // pipeline shape is shared (it's the same `compiler_pipeline::run`)
    // so the `stage` slug is interchangeable between check/parse.
    let stage_slug = payload["stage"].as_str().unwrap_or("ir_generate");
    telemetry.record_check(stage_slug, is_error);
    Ok(json!({
        "content": [
            { "type": "text", "text": serde_json::to_string_pretty(&payload).unwrap() }
        ],
        "isError": is_error,
    }))
}

// ─── axon.compose ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ComposeArgs {
    intent: String,
    /// Optional explicit domain override. When present it short-
    /// circuits the classifier; when absent the keyword scoreboard
    /// picks. Strings are normalised through
    /// [`compose::parse_domain_hint`] which accepts canonical names
    /// AND common aliases (`hc`, `fintech`, `rag`, …).
    #[serde(default)]
    domain: Option<String>,
}

fn compose_tool(
    args: Value,
    catalog: &Arc<Catalog>,
    telemetry: &Arc<Telemetry>,
) -> Result<Value, JsonRpcError> {
    let args: ComposeArgs = serde_json::from_value(args)
        .map_err(|e| JsonRpcError::invalid_params(format!("axon.compose: {e}")))?;

    // Resolve the optional domain hint into the closed enum. An
    // unknown string is a structured invalid_params — the
    // tools/list inputSchema already enumerated the closed catalog,
    // so the only way to land here is the agent invented a value.
    let domain_override = match args.domain.as_deref() {
        Some(s) => match compose::parse_domain_hint(s) {
            Some(d) => Some(d),
            None => {
                return Err(JsonRpcError::invalid_params(format!(
                    "axon.compose: unknown domain `{s}` — see axon.compose tool \
                     inputSchema for the closed 33-entry catalog (verticals + \
                     agent patterns + application patterns + meta-patterns)."
                )))
            }
        },
        None => None,
    };

    let r = compose::compose(&args.intent, domain_override, catalog).map_err(|e| {
        // Reaching this branch means the catalog is missing a template
        // for the closed enum — an integration-test regression.
        JsonRpcError {
            code: -32603,
            message: format!("axon.compose internal error: {e}"),
            data: None,
        }
    })?;

    // `axon_check_verdict != "well-formed"` would indicate the
    // template drifted from the parser without the integration test
    // catching it. Flip `isError` so the agent's reflex fires.
    let is_error = r.axon_check_verdict != "well-formed";
    // v1.3.0 — record the per-domain compose outcome. `intent` is
    // NEVER forwarded (privacy invariant #2); only the closed-catalog
    // `domain` slug, the top classifier score, and whether the call
    // carried an explicit `domain:` override.
    let top_score = r
        .alternatives
        .first()
        .map(|a| a.score)
        .unwrap_or(0);
    telemetry.record_compose(r.domain.slug(), top_score, domain_override.is_some());
    let payload = compose::response_to_json(&r);
    Ok(json!({
        "content": [
            { "type": "text", "text": serde_json::to_string_pretty(&payload).unwrap() }
        ],
        "isError": is_error,
    }))
}

// ─── axon.examples ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct ExamplesArgs {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    primitive: Option<String>,
}

/// Phase 9 — `axon.examples` handler. Resolves three independent
/// filter modes:
///
/// - `name:` — single-example lookup; returns the full `.axon`
///   `source` so the agent can paste / `axon.check` directly. An
///   unknown name is a structured `-32602` invalid_params (the
///   inputSchema does not enumerate slugs, so a typo is the only
///   way to land here).
/// - `topic:` — listing filtered by the closed `ExampleTopic` enum.
///   An unknown slug is `-32602` (the inputSchema's enum already
///   listed the 10 valid values).
/// - `primitive:` — listing filtered by primitive name. Free-form
///   (the inputSchema does not enumerate the 45 primitives by name);
///   an unknown primitive yields a zero-result response — NOT an
///   error — so the agent can iterate.
///
/// Filters compose with AND semantics: `topic: composition + primitive:
/// weave` returns examples that are BOTH on the composition topic AND
/// exercise `weave`. Omitting every filter returns the full corpus
/// listing without `source` bodies (the listing surface).
fn examples(
    args: Value,
    catalog: &Arc<Catalog>,
    telemetry: &Arc<Telemetry>,
) -> Result<Value, JsonRpcError> {
    let args: ExamplesArgs = if args.is_null() {
        ExamplesArgs::default()
    } else {
        serde_json::from_value(args)
            .map_err(|e| JsonRpcError::invalid_params(format!("axon.examples: {e}")))?
    };

    // Validate the topic (if any) against the closed enum BEFORE any
    // catalog lookup. An invented slug is a typed invalid_params, not
    // a silent zero-result response.
    let topic_filter: Option<ExampleTopic> = match args.topic.as_deref() {
        Some(s) => match ExampleTopic::parse(s) {
            Some(t) => Some(t),
            None => {
                return Err(JsonRpcError::invalid_params(format!(
                    "axon.examples: unknown topic `{s}` — see tool inputSchema for the \
                     closed 10-entry catalog (composition, session_types, shields, effects, \
                     streaming, data, agents, endpoints, memory, validation)."
                )))
            }
        },
        None => None,
    };

    // Single-example lookup wins over any filter — the agent already
    // knows the exact slug, so we return it verbatim with full source.
    if let Some(name) = args.name.as_deref() {
        let e = catalog.example(name).ok_or_else(|| JsonRpcError {
            code: -32602,
            message: format!(
                "axon.examples: unknown example `{name}` — call axon.examples without \
                 arguments to see the full catalog"
            ),
            data: None,
        })?;
        telemetry.record_examples(
            Some(name),
            topic_filter.map(|t| t.as_str()),
            args.primitive.as_deref(),
            1,
        );
        let payload = json!({
            "count": 1,
            "examples": [example_to_json(e, /*include_source=*/ true)],
        });
        return Ok(mcp_text_result(&serde_json::to_string_pretty(&payload).unwrap()));
    }

    // Listing path — apply both filters (AND semantics) over the full
    // corpus and return entries without `source` to keep the payload
    // bounded. The agent can drill down via `name:` on any hit.
    let primitive_filter = args.primitive.as_deref();
    let entries: Vec<&_> = catalog
        .examples()
        .filter(|e| match topic_filter {
            Some(t) => e.topic == t,
            None => true,
        })
        .filter(|e| match primitive_filter {
            Some(p) => e.primitives.iter().any(|x| x == p),
            None => true,
        })
        .collect();

    telemetry.record_examples(
        None,
        topic_filter.map(|t| t.as_str()),
        primitive_filter,
        entries.len(),
    );

    let examples_json: Vec<Value> = entries
        .iter()
        .map(|e| example_to_json(e, /*include_source=*/ false))
        .collect();
    Ok(mcp_text_result(&serde_json::to_string_pretty(&json!({
        "count": examples_json.len(),
        "examples": examples_json,
    })).unwrap()))
}

/// Project an [`Example`] into the wire shape. `include_source: true`
/// emits the raw `.axon` body (single-example resolution); `false`
/// omits it (listing — keeps the payload bounded so an agent can
/// scan the full catalog before drilling down).
fn example_to_json(e: &crate::knowledge::Example, include_source: bool) -> Value {
    if include_source {
        json!({
            "name": e.name,
            "title": e.title,
            "summary": e.summary,
            "topic": e.topic.as_str(),
            "primitives": e.primitives,
            "source": e.source,
        })
    } else {
        json!({
            "name": e.name,
            "title": e.title,
            "summary": e.summary,
            "topic": e.topic.as_str(),
            "primitives": e.primitives,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn catalog_with(name: &str, top: bool, cat: Category) -> Arc<Catalog> {
        // Build a real Catalog by writing one md file to a tempdir and
        // loading from it — exercises the same path the server takes
        // at startup. The dir name carries a monotonic counter so
        // parallel test invocations + repeated test runs never collide.
        use std::io::Write;
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "axon-emcp-toolstest-{}-{n}-{name}",
            std::process::id(),
        ));
        // Wipe any stale corpus from a prior run before writing.
        let _ = std::fs::remove_dir_all(&dir);
        let prims = dir.join("primitives");
        std::fs::create_dir_all(&prims).unwrap();
        let mut f = std::fs::File::create(prims.join(format!("{name}.md"))).unwrap();
        let body = format!(
            "---\nname: {name}\nsummary: test summary\ncategory: {}\ntop_level: {}\n\
             since: cycle X\ngrammar: |\n {name} ...\n---\n\nBody.\n",
            cat.as_str(),
            top,
        );
        f.write_all(body.as_bytes()).unwrap();
        Arc::new(Catalog::load_from(&dir).unwrap())
    }

    /// Throwaway telemetry registry for dispatcher tests — JSONL sink
    /// disabled, deployment ID empty. Cheap to construct per test.
    fn tel() -> Arc<Telemetry> {
        Arc::new(Telemetry::new(crate::telemetry::TelemetryConfig {
            jsonl_sink: None,
            deployment_id: "".into(),
            max_samples: 1000,
        }))
    }

    #[tokio::test]
    async fn primitives_returns_every_entry_when_unfiltered() {
        let cat = catalog_with("socket", true, Category::SessionTypes);
        let v = dispatch_call(
            json!({ "name": "axon.primitives", "arguments": {} }),
            &cat, &tel())
        .await
        .unwrap();
        let text = v["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["count"], 1);
        assert_eq!(parsed["primitives"][0]["name"], "socket");
        assert_eq!(parsed["primitives"][0]["top_level"], true);
    }

    #[tokio::test]
    async fn primitives_filters_by_category() {
        let cat = catalog_with("socket", true, Category::SessionTypes);
        let v = dispatch_call(
            json!({ "name": "axon.primitives",
                    "arguments": { "category": "session_types" } }),
            &cat, &tel())
        .await
        .unwrap();
        let parsed: Value =
            serde_json::from_str(v["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(parsed["count"], 1);
        // Filter that excludes:
        let v2 = dispatch_call(
            json!({ "name": "axon.primitives",
                    "arguments": { "category": "cognition" } }),
            &cat, &tel())
        .await
        .unwrap();
        let parsed2: Value =
            serde_json::from_str(v2["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(parsed2["count"], 0);
    }

    #[tokio::test]
    async fn primitives_rejects_unknown_category() {
        let cat = catalog_with("socket", true, Category::SessionTypes);
        let err = dispatch_call(
            json!({ "name": "axon.primitives",
                    "arguments": { "category": "bogus" } }),
            &cat, &tel())
        .await
        .expect_err("must reject");
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("unknown category"));
    }

    #[tokio::test]
    async fn primitive_doc_returns_full_metadata_and_body() {
        let cat = catalog_with("socket", true, Category::SessionTypes);
        let v = dispatch_call(
            json!({ "name": "axon.primitive_doc",
                    "arguments": { "name": "socket" } }),
            &cat, &tel())
        .await
        .unwrap();
        let text = v["content"][0]["text"].as_str().unwrap();
        let payload: Value = serde_json::from_str(text).unwrap();
        assert_eq!(payload["name"], "socket");
        assert_eq!(payload["top_level"], true);
        assert!(payload["body_markdown"].as_str().unwrap().contains("Body."));
        assert!(payload["grammar"].as_str().unwrap().contains("socket"));
    }

    #[tokio::test]
    async fn primitive_doc_rejects_unknown_name() {
        let cat = catalog_with("socket", true, Category::SessionTypes);
        let err = dispatch_call(
            json!({ "name": "axon.primitive_doc",
                    "arguments": { "name": "does_not_exist" } }),
            &cat, &tel())
        .await
        .expect_err("must reject");
        assert!(err.message.contains("unknown primitive"));
        assert!(err.message.contains("axon.primitives"));
    }

    #[tokio::test]
    async fn unknown_tool_returns_method_not_found() {
        let cat = catalog_with("socket", true, Category::SessionTypes);
        let err = dispatch_call(
            json!({ "name": "axon.does_not_exist", "arguments": {} }),
            &cat, &tel())
        .await
        .expect_err("must reject");
        assert_eq!(err.code, -32601);
    }

    // ── Phase 1: axon.check ─────────────────────────────────────────────

    #[tokio::test]
    async fn check_returns_ok_for_a_well_formed_program() {
        let cat = catalog_with("socket", true, Category::SessionTypes);
        let v = dispatch_call(
            json!({
                "name": "axon.check",
                "arguments": { "source": "persona X { tone: precise }" }
            }),
            &cat, &tel())
        .await
        .unwrap();
        assert_eq!(v["isError"], false);
        let payload: Value =
            serde_json::from_str(v["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["errors"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn check_returns_diagnostic_with_isError_on_syntax_garbage() {
        let cat = catalog_with("socket", true, Category::SessionTypes);
        let v = dispatch_call(
            json!({
                "name": "axon.check",
                "arguments": { "source": "@@@" }
            }),
            &cat, &tel())
        .await
        .unwrap();
        // `isError` MUST flip — agents key off this for the "go fix it"
        // reflex; a malformed program is a typed failure, not a tool fault.
        assert_eq!(v["isError"], true);
        let payload: Value =
            serde_json::from_str(v["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload["ok"], false);
        let errors = payload["errors"].as_array().unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0]["severity"], "error");
        // The stage is whichever the pipeline failed at (lex or parse —
        // depends on how `@@@` is classified). Either is acceptable, but
        // it must be one of the closed catalog values.
        let stage = errors[0]["stage"].as_str().unwrap();
        assert!(matches!(stage, "lex" | "parse"));
        assert!(errors[0]["line"].as_u64().unwrap() >= 1);
    }

    #[tokio::test]
    async fn check_rejects_missing_source_argument() {
        let cat = catalog_with("socket", true, Category::SessionTypes);
        let err = dispatch_call(
            json!({ "name": "axon.check", "arguments": {} }),
            &cat, &tel())
        .await
        .expect_err("missing required `source` must reject");
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("axon.check"));
    }

    #[tokio::test]
    async fn check_accepts_optional_filename_for_diagnostics() {
        let cat = catalog_with("socket", true, Category::SessionTypes);
        // The filename does not affect the diagnostic shape we surface
        // (we don't pipe source_snippet through to the agent yet) but
        // it must be accepted as an optional argument.
        let v = dispatch_call(
            json!({
                "name": "axon.check",
                "arguments": {
                    "source": "persona X { tone: precise }",
                    "filename": "my_draft.axon"
                }
            }),
            &cat, &tel())
        .await
        .unwrap();
        assert_eq!(v["isError"], false);
    }

    // ── Phase 1: axon.parse ─────────────────────────────────────────────

    #[tokio::test]
    async fn parse_returns_ir_for_a_well_formed_program() {
        let cat = catalog_with("socket", true, Category::SessionTypes);
        let v = dispatch_call(
            json!({
                "name": "axon.parse",
                "arguments": { "source": "persona X { tone: precise }" }
            }),
            &cat, &tel())
        .await
        .unwrap();
        assert_eq!(v["isError"], false);
        let payload: Value =
            serde_json::from_str(v["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["stage"], "ir_generate");
        // The IR's root is a `Program` node. The agent uses this to
        // confirm the file's top-level shape before composing more.
        assert_eq!(payload["ir"]["node_type"], "program");
        // Personas declared at top-level land in the `personas` array.
        let personas = payload["ir"]["personas"].as_array().unwrap();
        assert_eq!(personas.len(), 1);
        assert_eq!(personas[0]["name"], "X");
    }

    #[tokio::test]
    async fn parse_returns_same_diagnostic_shape_as_check_on_failure() {
        let cat = catalog_with("socket", true, Category::SessionTypes);
        let v = dispatch_call(
            json!({
                "name": "axon.parse",
                "arguments": { "source": "@@@" }
            }),
            &cat, &tel())
        .await
        .unwrap();
        assert_eq!(v["isError"], true);
        let payload: Value =
            serde_json::from_str(v["content"][0]["text"].as_str().unwrap()).unwrap();
        // The failure shape is uniform with axon.check — same keys,
        // same severity vocabulary. (parse just adds `ir` on success.)
        assert_eq!(payload["ok"], false);
        assert!(payload["errors"].as_array().unwrap().len() >= 1);
        assert!(payload["ir"].is_null());
    }

    // ── Phase 4: axon.compose ───────────────────────────────────────────

    /// Build a catalog with the embedded corpus — needed for compose
    /// since it depends on the real `templates/` shipped under
    /// `src/knowledge/templates/`. The other tools' tests use a
    /// stub corpus, but compose's substantive behaviour is the
    /// classifier + template emission, both of which require the
    /// real corpus.
    fn embedded_catalog() -> Arc<Catalog> {
        Arc::new(Catalog::load_embedded().expect("embedded corpus must load"))
    }

    #[tokio::test]
    async fn compose_returns_well_formed_scaffold_for_healthcare_intent() {
        let cat = embedded_catalog();
        let v = dispatch_call(
            json!({
                "name": "axon.compose",
                "arguments": { "intent": "a patient summarisation service with PHI" }
            }),
            &cat, &tel())
        .await
        .unwrap();
        assert_eq!(v["isError"], false);
        let payload: Value =
            serde_json::from_str(v["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload["domain"], "healthcare");
        assert_eq!(payload["axon_check_verdict"], "well-formed");
        // The scaffold must mention HIPAA — that's the wire signal
        // a downstream agent uses to know the compliance is real.
        assert!(payload["scaffold"].as_str().unwrap().contains("HIPAA"));
        // Compliance applied is structured + agent-machine-readable.
        let compl: Vec<&str> = payload["compliance_applied"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(compl.contains(&"HIPAA"));
    }

    #[tokio::test]
    async fn compose_honors_explicit_domain_argument() {
        let cat = embedded_catalog();
        // Intent matches banking; force the chat template instead.
        let v = dispatch_call(
            json!({
                "name": "axon.compose",
                "arguments": {
                    "intent": "process credit card payments and loan applications",
                    "domain": "chat"
                }
            }),
            &cat, &tel())
        .await
        .unwrap();
        assert_eq!(v["isError"], false);
        let payload: Value =
            serde_json::from_str(v["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload["domain"], "chat");
        assert_eq!(payload["axon_check_verdict"], "well-formed");
    }

    #[tokio::test]
    async fn compose_falls_back_to_generic_for_unrelated_intent() {
        let cat = embedded_catalog();
        let v = dispatch_call(
            json!({
                "name": "axon.compose",
                "arguments": { "intent": "say hello" }
            }),
            &cat, &tel())
        .await
        .unwrap();
        let payload: Value =
            serde_json::from_str(v["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload["domain"], "generic");
        assert_eq!(payload["axon_check_verdict"], "well-formed");
    }

    #[tokio::test]
    async fn compose_rejects_unknown_domain_hint() {
        let cat = embedded_catalog();
        let err = dispatch_call(
            json!({
                "name": "axon.compose",
                "arguments": {
                    "intent": "anything",
                    "domain": "bogus-domain"
                }
            }),
            &cat, &tel())
        .await
        .expect_err("unknown domain must be a structured invalid_params");
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("unknown domain"));
    }

    #[tokio::test]
    async fn compose_rejects_missing_intent_argument() {
        let cat = embedded_catalog();
        let err = dispatch_call(
            json!({ "name": "axon.compose", "arguments": {} }),
            &cat, &tel())
        .await
        .expect_err("missing required `intent` must reject");
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("axon.compose"));
    }

    #[tokio::test]
    async fn compose_response_carries_explainability_scoreboard() {
        let cat = embedded_catalog();
        let v = dispatch_call(
            json!({
                "name": "axon.compose",
                "arguments": { "intent": "patient PHI clinical trial under HIPAA" }
            }),
            &cat, &tel())
        .await
        .unwrap();
        let payload: Value =
            serde_json::from_str(v["content"][0]["text"].as_str().unwrap()).unwrap();
        let alts = payload["alternatives"].as_array().unwrap();
        // We always return the full scoreboard (one entry per domain
        // in `Domain::all()`) so the agent can quote it. v1.2.0
        // grew the catalogue to 12; v1.2.0 to 20; v1.2.0
        // closes the cycle at 33. The assertion tracks the count
        // exactly so any future drop / addition surfaces here.
        assert_eq!(alts.len(), 33);
        assert!(alts[0]["score"].as_u64().unwrap() >= 1);
        assert_eq!(alts[0]["domain"], "healthcare");
        // next_steps + primitives_used surface a curated checklist.
        assert!(!payload["next_steps"].as_array().unwrap().is_empty());
        assert!(!payload["primitives_used"].as_array().unwrap().is_empty());
    }

    // ── Phase 9: axon.examples ──────────────────────────────────────────

    #[tokio::test]
    async fn examples_unfiltered_returns_full_corpus() {
        let cat = embedded_catalog();
        let v = dispatch_call(
            json!({ "name": "axon.examples", "arguments": {} }),
            &cat, &tel())
        .await
        .unwrap();
        assert_eq!(v["isError"], false);
        let payload: Value =
            serde_json::from_str(v["content"][0]["text"].as_str().unwrap()).unwrap();
        // Phase 9 ships 25 curated examples (5 composition [+ v2.8.0
        // tool_structured_args] + v2.4.0 quant_feature_map + v2.46.0
        // temporal_cognitive_context + 2 session_types + 1 shields +
        // 1 effects + 1 streaming + 5 data [+ v2.48.0
        // secret_custody_rotation + v2.49.0 secret_partition_multitenant +
        // v2.60.0 governed_crm_delivery] + 2 agents + 4 endpoints [+ v2.38.0
        // cors_named_origin_policy + v2.46.0 widget_ephemeral_credential +
        // v2.62.0 query_safe_search] + 1 memory + 2 validation +
        // v2.63.0 governed_data_pipeline [data] + v2.76.0
        // module_imports [composition — the EMS]).
        assert_eq!(payload["count"], 30);
        assert_eq!(payload["examples"].as_array().unwrap().len(), 30);
        // Listing path omits `source` — keeps the payload bounded.
        let first = &payload["examples"][0];
        assert!(first["name"].is_string());
        assert!(first["title"].is_string());
        assert!(first["summary"].is_string());
        assert!(first["topic"].is_string());
        assert!(first["primitives"].is_array());
        assert!(
            first["source"].is_null(),
            "listing must omit `source` to keep the payload bounded"
        );
    }

    #[tokio::test]
    async fn examples_by_name_returns_full_source() {
        let cat = embedded_catalog();
        let v = dispatch_call(
            json!({
                "name": "axon.examples",
                "arguments": { "name": "weave_braid" }
            }),
            &cat, &tel())
        .await
        .unwrap();
        assert_eq!(v["isError"], false);
        let payload: Value =
            serde_json::from_str(v["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload["count"], 1);
        let entry = &payload["examples"][0];
        assert_eq!(entry["name"], "weave_braid");
        assert_eq!(entry["topic"], "composition");
        // Single-example resolution INCLUDES the source — that is the
        // agent's primary use case (paste / `axon.check`).
        let source = entry["source"].as_str().expect("source must be present");
        assert!(source.contains("weave {"));
    }

    #[tokio::test]
    async fn examples_filters_by_topic() {
        let cat = embedded_catalog();
        let v = dispatch_call(
            json!({
                "name": "axon.examples",
                "arguments": { "topic": "session_types" }
            }),
            &cat, &tel())
        .await
        .unwrap();
        let payload: Value =
            serde_json::from_str(v["content"][0]["text"].as_str().unwrap()).unwrap();
        // Phase 9 ships 2 session-types examples (session duality + socket
        // websocket binding). Every returned entry must carry the topic.
        assert_eq!(payload["count"], 2);
        for e in payload["examples"].as_array().unwrap() {
            assert_eq!(e["topic"], "session_types");
        }
    }

    #[tokio::test]
    async fn examples_filters_by_primitive() {
        let cat = embedded_catalog();
        let v = dispatch_call(
            json!({
                "name": "axon.examples",
                "arguments": { "primitive": "weave" }
            }),
            &cat, &tel())
        .await
        .unwrap();
        let payload: Value =
            serde_json::from_str(v["content"][0]["text"].as_str().unwrap()).unwrap();
        // `weave_braid` is the only example that lists `weave`.
        assert_eq!(payload["count"], 1);
        assert_eq!(payload["examples"][0]["name"], "weave_braid");
    }

    #[tokio::test]
    async fn examples_topic_and_primitive_filters_compose_with_and_semantics() {
        let cat = embedded_catalog();
        let v = dispatch_call(
            json!({
                "name": "axon.examples",
                "arguments": {
                    "topic":     "composition",
                    "primitive": "weave"
                }
            }),
            &cat, &tel())
        .await
        .unwrap();
        let payload: Value =
            serde_json::from_str(v["content"][0]["text"].as_str().unwrap()).unwrap();
        // Both filters hit weave_braid (composition + uses weave).
        assert_eq!(payload["count"], 1);
        assert_eq!(payload["examples"][0]["name"], "weave_braid");
    }

    #[tokio::test]
    async fn examples_rejects_unknown_topic_with_structured_error() {
        let cat = embedded_catalog();
        let err = dispatch_call(
            json!({
                "name": "axon.examples",
                "arguments": { "topic": "not-a-topic" }
            }),
            &cat, &tel())
        .await
        .expect_err("unknown topic must reject");
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("unknown topic"));
    }

    #[tokio::test]
    async fn examples_rejects_unknown_name_with_structured_error() {
        let cat = embedded_catalog();
        let err = dispatch_call(
            json!({
                "name": "axon.examples",
                "arguments": { "name": "does_not_exist" }
            }),
            &cat, &tel())
        .await
        .expect_err("unknown example name must reject");
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("unknown example"));
    }

    #[tokio::test]
    async fn examples_unknown_primitive_returns_empty_listing_not_error() {
        let cat = embedded_catalog();
        // Primitive filter is free-form (the inputSchema does not enumerate
        // every primitive name) — an unknown primitive must yield zero
        // results, NOT a structured error. The agent can iterate.
        let v = dispatch_call(
            json!({
                "name": "axon.examples",
                "arguments": { "primitive": "no_such_primitive" }
            }),
            &cat, &tel())
        .await
        .unwrap();
        let payload: Value =
            serde_json::from_str(v["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload["count"], 0);
        assert!(payload["examples"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn examples_advertised_in_tools_list() {
        let names: Vec<String> = list()
            .iter()
            .map(|v| v["name"].as_str().unwrap().to_string())
            .collect();
        assert!(
            names.contains(&"axon.examples".to_string()),
            "axon.examples must be advertised in tools/list; saw {names:?}"
        );
    }

    #[tokio::test]
    async fn compose_advertised_in_tools_list() {
        let names: Vec<String> = list()
            .iter()
            .map(|v| v["name"].as_str().unwrap().to_string())
            .collect();
        assert!(
            names.contains(&"axon.compose".to_string()),
            "axon.compose must be advertised in tools/list; saw {names:?}"
        );
    }
}
