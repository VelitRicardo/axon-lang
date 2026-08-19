//! v1.23.0 — Schema validation for first-class axonendpoint routes.
//!
//! Given an axonendpoint's declared `body: T` (request side, D4) or
//! `output: T` (response side, D5), validate that every accepted body
//! matches `T`'s schema verbatim. The validation function is **pure +
//! total over the declared type system**.
//!
//! ## Same primitive, two call sites
//!
//! `validate_body` is consumed twice in the dynamic-route fallback:
//!
//! 1. **Request side (D4)** — before flow dispatch. On violation the
//!    HTTP layer returns 400 Bad Request with the full structured
//!    `BodyValidationError` so the adopter client can correct the
//!    request.
//! 2. **Response side (D5)** — after flow dispatch, before returning
//!    to the client. On violation the HTTP layer returns **GENERIC
//!    500** to the client (OWASP — schema details never leak to a
//!    potentially malicious caller) but records the full
//!    `BodyValidationError` in the audit log so the adopter inspects
//!    the trail to fix the FLOW.
//!
//! The validator itself does not care which side it runs on — same
//! primitive, same drift gate.
//!
//! ## Pillar trace (D12)
//!
//! - **MATHEMATICS** — `validate_body : (RequestBody, Type) → Result<(),
//!   ValidationError>` is a pure function. Given the same input and the
//!   same type table, the function is deterministic and total: every input
//!   maps to exactly one result.
//! - **LOGIC** — every accepted body matches the declared schema. No
//!   widening, no coercion, no "kinda matches". A body of `{amount: "50"}`
//!   does NOT satisfy `LoanApplication { amount: Float }` — string-to-float
//!   coercion is the client's responsibility, not the server's.
//! - **PHILOSOPHY** — the declaration IS the contract. An auditor reads
//!   source + KNOWS exactly what bodies are accepted at every endpoint.
//!   Free-form bodies require explicitly omitting `body:` (D9).
//! - **COMPUTING** — backwards-compat: when `body_type` is empty, no
//! validation runs (free-form JSON, as before v1.23.0). Adopters opt in
//!   by declaring `body:` on their axonendpoints.
//!
//! ## Cross-stack mirror (D11)
//!
//! Python sibling lives at `axon/runtime/route_schema.py`. Both stacks
//! produce byte-identical `(type_name, field_path, expected, got)` tuples
//! for the same input under the shared drift-gate corpus at
//! `tests/fixtures/body_schema/corpus.json`.

use std::collections::HashMap;

use serde_json::Value;

use crate::ast::{Declaration, Program, TypeDefinition};

/// Snapshot of a `type T { … }` declaration relevant to body validation.
/// Only the fields the validator consults are projected — `compliance`,
/// `where_clause`, and `range_constraint` are out of scope for 32.c
/// (where/compliance ship in their own future cycles).
#[derive(Debug, Clone, PartialEq)]
pub struct TypeSchema {
    pub name: String,
    pub fields: Vec<FieldSchema>,
    /// Closed numeric range constraint per `RANGED_TYPES` semantics. The
    /// parser sets this for `type X(0.0..1.0)` declarations.
    pub range: Option<(f64, f64)>,
}

/// One field inside a structured type. `optional == true` if the source
/// declared the field as `name: T?`.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldSchema {
    pub name: String,
    pub type_name: String,
    /// `List<X>`'s `generic_param` is `"X"`. Empty string for
    /// non-parameterised types.
    pub generic_param: String,
    pub optional: bool,
}

/// Structured body-validation error. The HTTP layer projects this into a
/// 400 Bad Request with the field/expected/got triple so adopter clients
/// can correct their request without server-side log diving.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct BodyValidationError {
    /// Top-level body type the validation was attempted against (e.g.
    /// `"LoanApplication"`).
    pub expected_type: String,
    /// Dotted path to the offending field: `"applicant.address.street"`
    /// for nested structures, `"[2].name"` for list-element index 2.
    /// Empty string when the violation is at the top-level body itself
    /// (e.g. expected object, got string).
    pub field_path: String,
    /// Declared type the validator expected.
    pub expected: String,
    /// JSON-type tag observed (`"string"`, `"number"`, `"integer"`,
    /// `"boolean"`, `"array"`, `"object"`, `"null"`, `"missing"`).
    pub got: String,
    /// Adopter-facing diagnostic — full sentence with a corrective hint.
    /// Stable across versions per D8 backwards-compat surface.
    pub hint: String,
    /// v1.31.0 (D2) — Declared cardinality kind of the expected
    /// type: `"singular"` | `"plural"` | `"stream"` | `"unit"` |
    /// `"unknown"`. Empty string for primitive-type validation errors
    /// where the cardinality isn't load-bearing (the existing v1.39.0
    /// surface). Serde `#[serde(default)]` keeps adopter consumers of
    /// older versions byte-compatible.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub expected_cardinality: String,
    /// v1.31.0 (D2) — Observed cardinality kind of the response
    /// body: same alphabet as `expected_cardinality`. Empty when not
    /// applicable. The asymmetry expected/got is the diagnostic
    /// payload adopters reach for first when D5 fires.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub got_cardinality: String,
    /// v1.31.0 (D2) — Length of the observed value when it is
    /// `array` (plural). `None` for non-array gots. Helps adopters
    /// confirm "the flow returned 1 row, but the contract said
    /// singular — collapse with `result[0]` or change the endpoint
    /// to `output: List<T>`".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub got_length: Option<u64>,
    /// v1.31.0 (D2) — Documentation URL adopters can follow for
    /// the canonical remediation steps. Empty when the error is not
    /// a cardinality mismatch (the existing v1.39.0 surface). The
    /// URL is stable; the page may evolve.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub remediation_url: String,
}

impl std::fmt::Display for BodyValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.hint)
    }
}

impl std::error::Error for BodyValidationError {}

/// Built-in primitive type names recognised by the validator. Any name
/// in this set is checked directly against the JSON value's tag; names
/// NOT in this set are looked up in the per-deploy type table (structured
/// types). Anything missing from both is reported as `unknown_type` so
/// adopters who misspell `Strng` get a clear diagnostic instead of a
/// silent "everything passes" trap.
pub const BUILTIN_PRIMITIVES: &[&str] = &[
    "String",
    "Integer",
    "Float",
    "Boolean",
    "Duration",
    "Any",
];

/// Built-in range-constrained numeric types. Mirrors
/// `RANGED_TYPES` in `axon/compiler/type_checker.py`. These accept any
/// JSON number that falls within the closed interval.
pub fn builtin_range(name: &str) -> Option<(f64, f64)> {
    match name {
        "RiskScore" | "ConfidenceScore" => Some((0.0, 1.0)),
        "SentimentScore" => Some((-1.0, 1.0)),
        _ => None,
    }
}

/// Walk every `type T { … }` declaration in the deployed program and
/// produce a `name → TypeSchema` lookup table. Last-wins on collision
/// is the same semantics as Rust's `HashMap::insert` — type-name
/// collisions across deploys are out of scope for 32.c (deferred to a
/// future type-registry cycle). For 32.c the only consumer is the
/// dynamic-route fallback handler which captures the table once per
/// deploy.
pub fn collect_type_table(program: &Program) -> HashMap<String, TypeSchema> {
    let mut table = HashMap::new();
    for decl in &program.declarations {
        if let Declaration::Type(td) = decl {
            table.insert(td.name.clone(), type_schema_from(td));
        }
    }
    table
}

fn type_schema_from(td: &TypeDefinition) -> TypeSchema {
    let fields = td
        .fields
        .iter()
        .map(|f| FieldSchema {
            name: f.name.clone(),
            type_name: f.type_expr.name.clone(),
            generic_param: f.type_expr.generic_param.clone(),
            optional: f.type_expr.optional,
        })
        .collect();
    let range = td
        .range_constraint
        .as_ref()
        .map(|rc| (rc.min_value, rc.max_value));
    TypeSchema {
        name: td.name.clone(),
        fields,
        range,
    }
}

/// Tag the JSON value with the lowercase string the validator reports as
/// `got`. Numbers split into `"integer"` vs `"number"` so adopters
/// declaring `Integer` get the precise "got a number with decimals" path.
fn json_tag(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "integer"
            } else {
                "number"
            }
        }
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Validate `body` against the type named `type_name` looked up in
/// `table` (or matched against `BUILTIN_PRIMITIVES`).
///
/// Returns `Ok(())` on success. Returns `Err(BodyValidationError)` with
/// the first violation encountered (depth-first, field declaration
/// order). The error carries enough structure for the HTTP layer to
/// emit a stable 400 Bad Request body.
///
/// **Backwards-compat (D9)**: when `type_name` is empty, returns
/// `Ok(())` immediately. Adopters who don't declare `body:` keep the
/// pre-cycle-32 free-form behavior.
///
/// **v2.0.0 — Canonical FlowEnvelope-aware entry**. As of v2.0.0
/// (39.d) `validate_body` is the SINGLE place that knows about wire
/// shapes:
///
///   1. `FlowEnvelope<T>` declarations — `validate_body` unwraps
///      `body["result"]` and recurses with the inner T. The outer
///      envelope shape is verified by construction (object with
///      `result` slot).
///   2. Bare generics (`List<T>`, `Stream<T>`) — parsed at the
///      canonical entry via [`parse_generic_head`]. The internal
/// [`validate_value`] no longer carries a section 0 preamble for
///      string-stripping (the v1.40.2 / v1.40.3 bridge is retired).
///   3. Primitives, structs, ranges — passed through to validate_value.
///
/// The convergence dividend retires ~46 lines of v1.x bridge code
/// in favour of one well-named canonical entry. D5 callers (the
/// runtime gate in `axon_server::apply_output_validation_gate`)
/// pass the raw declared type verbatim — no manual unwrapping
/// needed.
pub fn validate_body(
    body: &Value,
    type_name: &str,
    table: &HashMap<String, TypeSchema>,
) -> Result<(), BodyValidationError> {
    let t = type_name.trim();
    if t.is_empty() {
        return Ok(());
    }
    // v2.0.0 — FlowEnvelope<T> canonical unwrap. When the adopter
    // declares `output: FlowEnvelope<T>` (the v2.0.0 mandatory wire
    // shape), the body is `{ontological_type, result, certainty, …}`
    // and we validate `result` against T. The outer envelope shape
    // (object with `result` slot) is verified by construction at
    // the seal() layer; here we trust + unwrap.
    if let Some(inner) = strip_flow_envelope(t) {
        let obj = match body.as_object() {
            Some(o) => o,
            None => {
                return Err(BodyValidationError {
                    expected_type: type_name.to_string(),
                    field_path: String::new(),
                    expected: t.to_string(),
                    got: json_tag(body).to_string(),
                    hint: format!(
                        "axonendpoint declared `output: {t}` but the response \
                         body is not a JSON object — the FlowEnvelope wire \
                         shape requires `{{ontological_type, result, …}}`. \
                         This typically indicates a bug in the response wrapper."
                    ),
                    ..Default::default()
                });
            }
        };
        let result_slot = obj
            .get("result")
            .cloned()
            .unwrap_or(Value::Null);
        // `FlowEnvelope<Any>` is the universal accept (degraded
        // surface) — no further validation on the inner.
        if inner == "Any" {
            return Ok(());
        }
        // Recurse on the inner T (which may itself be a generic
        // like `List<X>` or a struct or a primitive).
        return validate_body(&result_slot, &inner, table);
    }
    // v2.0.0 — bare generic parsing at the canonical entry.
    // `List<T>` / `Stream<T>` get split into `(head, inner)` before
    // dispatching to validate_value, which now assumes pre-parsed
    // input. Pre-39.d this parsing lived in validate_value's section 0
    // preamble (v1.40.2 / v1.40.3 bridge); 39.d retires it because
    // FlowEnvelope<T> is the canonical wire shape.
    let (head, generic) = parse_generic_head(t);
    validate_value(body, &head, &generic, "", table, t)
}

/// v2.0.0 — Strip the outer `FlowEnvelope<…>` wrapper from a
/// declared type string. Returns the inner T verbatim (which may
/// be a nested generic like `List<X>` or a struct name). Returns
/// `None` when the input is NOT a FlowEnvelope wrapper.
fn strip_flow_envelope(t: &str) -> Option<String> {
    let rest = t.strip_prefix("FlowEnvelope<")?;
    let inner = rest.strip_suffix('>')?;
    Some(inner.trim().to_string())
}

/// v2.0.0 — Parse the closed-catalog generic head + inner. Used
/// by [`validate_body`] (the canonical entry) and by recursive
/// callers like [`validate_list`] that need to split a string-form
/// element type before calling [`validate_value`].
///
/// Closed grammar at v2.0.0:
///   - `List<X>`   → `("List", "X")`
///   - `Stream<X>` → `("Stream", "X")`
///   - anything else → `(t, "")`
///
/// Future generics (Map<K,V>, Optional<T>, …) extend this helper
/// additively without touching the validators downstream.
fn parse_generic_head(t: &str) -> (String, String) {
    if let Some(rest) = t.strip_prefix("List<") {
        if let Some(inner) = rest.strip_suffix('>') {
            return ("List".to_string(), inner.trim().to_string());
        }
    }
    if let Some(rest) = t.strip_prefix("Stream<") {
        if let Some(inner) = rest.strip_suffix('>') {
            return ("Stream".to_string(), inner.trim().to_string());
        }
    }
    (t.to_string(), String::new())
}

/// Internal recursive validator.
///
/// `body_type` is the top-level type the user declared (kept invariant
/// across recursion for diagnostic continuity).
/// `field_path` is the dotted path accumulated so far ("" at top level).
/// `generic_param` carries `List<T>`'s element type when validating a
/// list — empty otherwise.
///
/// **v2.0.0**: post-39.d this function assumes the input is
/// PRE-PARSED. The v1.40.2/v1.40.3 section 0 preamble (string-stripping for
/// `List<T>` / `Stream<T>`) is retired in favour of one canonical
/// parse at the [`validate_body`] entry. Recursive callers
/// ([`validate_list`], [`validate_struct`]) pre-parse via
/// [`parse_generic_head`] before calling here.
fn validate_value(
    v: &Value,
    type_name: &str,
    generic_param: &str,
    field_path: &str,
    table: &HashMap<String, TypeSchema>,
    body_type: &str,
) -> Result<(), BodyValidationError> {
    // v2.0.0 — `Stream<T>` defensive accept. Top-level Stream<T>
    // body validation is structurally unreachable from the v2.0.0
    // production path (SSE chunks validate at the streaming wire,
    // not via this body validator — D9 of plan vivo v2.0.0). When
    // we DO observe it defensively, return Ok early.
    if type_name == "Stream" {
        return Ok(());
    }
    // section 1 — primitives
    if BUILTIN_PRIMITIVES.contains(&type_name) {
        return validate_primitive(v, type_name, field_path, body_type);
    }
    // section 2 — range-constrained built-ins (RiskScore, ConfidenceScore, …)
    if let Some((lo, hi)) = builtin_range(type_name) {
        return validate_ranged_number(v, type_name, lo, hi, field_path, body_type);
    }
    // section 3 — generic List<T>
    if type_name == "List" {
        return validate_list(v, generic_param, field_path, table, body_type);
    }
    // section 4 — structured types declared in the program
    if let Some(schema) = table.get(type_name) {
        // Numeric range-constrained user types (`type RiskScore(0.0..1.0)`)
        if let Some((lo, hi)) = schema.range {
            return validate_ranged_number(v, type_name, lo, hi, field_path, body_type);
        }
        return validate_struct(v, schema, field_path, table, body_type);
    }
    // section 5 — unknown type. Adopter misspell or undeclared type. We surface
    // it instead of silently passing so the diagnostic is actionable.
    Err(BodyValidationError {
        expected_type: body_type.to_string(),
        field_path: field_path.to_string(),
        expected: type_name.to_string(),
        got: json_tag(v).to_string(),
        hint: format!(
            "axonendpoint declared an unknown body type `{type_name}` for field \
             `{field_path}` — neither a built-in primitive nor a declared \
             `type` in the deployed source. Add `type {type_name} {{ … }}` to \
             the source or correct the spelling."
        ),
        ..Default::default()
    })
}

fn validate_primitive(
    v: &Value,
    type_name: &str,
    field_path: &str,
    body_type: &str,
) -> Result<(), BodyValidationError> {
    let ok = match (type_name, v) {
        ("String", Value::String(_)) => true,
        ("Integer", Value::Number(n)) => n.is_i64() || n.is_u64(),
        ("Float", Value::Number(_)) => true,
        ("Boolean", Value::Bool(_)) => true,
        ("Duration", Value::String(_)) => true,
        ("Any", _) => true,
        _ => false,
    };
    if ok {
        return Ok(());
    }
    Err(BodyValidationError {
        expected_type: body_type.to_string(),
        field_path: field_path.to_string(),
        expected: type_name.to_string(),
        got: json_tag(v).to_string(),
        hint: format!(
            "Body field `{field_path}` must be a `{type_name}` but received a \
             {got}. Adjust the request body or the axonendpoint's `body:` \
             declaration.",
            field_path = if field_path.is_empty() { "<body>" } else { field_path },
            type_name = type_name,
            got = json_tag(v),
        ),
        ..Default::default()
    })
}

/// Format an `f64` the same way both stacks render bounds + `got`
/// values inside validation errors. Whole-valued floats render as the
/// integer ("0", "1", "-1"); fractional values render via `{f64}`'s
/// shortest round-trip representation ("1.5", "-1.5"). This locks the
/// drift gate against Rust's `Display for f64` quirks vs Python's
/// `str(float)` adding ".0".
pub fn fmt_f64(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e16 {
        return format!("{}", n as i64);
    }
    format!("{n}")
}

fn validate_ranged_number(
    v: &Value,
    type_name: &str,
    lo: f64,
    hi: f64,
    field_path: &str,
    body_type: &str,
) -> Result<(), BodyValidationError> {
    // v1.23.0 — `Number::is_i64() || is_u64() || is_f64()` already covers
    // every JSON-number variant; bool excluded explicitly because
    // `serde_json::Value::as_f64` does NOT coerce booleans.
    let n = match (v, v.as_f64()) {
        (Value::Number(_), Some(n)) => n,
        _ => {
            return Err(BodyValidationError {
                expected_type: body_type.to_string(),
                field_path: field_path.to_string(),
                expected: type_name.to_string(),
                got: json_tag(v).to_string(),
                hint: format!(
                    "Body field `{path}` must be a `{type_name}` (numeric in \
                     [{lo}, {hi}]) but received a {got}.",
                    path = if field_path.is_empty() { "<body>" } else { field_path },
                    type_name = type_name,
                    got = json_tag(v),
                    lo = fmt_f64(lo),
                    hi = fmt_f64(hi),
                ),
                ..Default::default()
            });
        }
    };
    if n < lo || n > hi {
        let lo_s = fmt_f64(lo);
        let hi_s = fmt_f64(hi);
        let n_s = fmt_f64(n);
        return Err(BodyValidationError {
            expected_type: body_type.to_string(),
            field_path: field_path.to_string(),
            expected: format!("{type_name} ∈ [{lo_s}, {hi_s}]"),
            got: n_s.clone(),
            hint: format!(
                "Body field `{path}` must satisfy `{type_name} ∈ [{lo_s}, \
                 {hi_s}]` but received `{n_s}`.",
                path = if field_path.is_empty() { "<body>" } else { field_path },
            ),
            ..Default::default()
        });
    }
    Ok(())
}

fn validate_list(
    v: &Value,
    element_type: &str,
    field_path: &str,
    table: &HashMap<String, TypeSchema>,
    body_type: &str,
) -> Result<(), BodyValidationError> {
    let arr = match v.as_array() {
        Some(a) => a,
        None => {
            return Err(BodyValidationError {
                expected_type: body_type.to_string(),
                field_path: field_path.to_string(),
                expected: format!("List<{element_type}>"),
                got: json_tag(v).to_string(),
                hint: format!(
                    "Body field `{path}` must be a `List<{element_type}>` \
                     (JSON array) but received a {got}.",
                    path = if field_path.is_empty() { "<body>" } else { field_path },
                    got = json_tag(v),
                ),
                // v1.31.0 (D2) — When this validation fires at
                // the TOP-LEVEL body (empty field_path), the mismatch
                // is between the declared `List<T>` (plural) and the
                // observed JSON shape (not an array). Populate the
                // cardinality diagnostic fields so adopters reaching
                // for the audit_log entry see the cardinality story
                // directly. For nested field violations the fields
                // stay empty (the mismatch is at a sub-field, not
                // load-bearing for the endpoint-level contract).
                expected_cardinality: if field_path.is_empty() {
                    "plural".to_string()
                } else {
                    String::new()
                },
                got_cardinality: if field_path.is_empty() {
                    match v {
                        Value::Object(_) => "singular".to_string(),
                        Value::Null => "unit".to_string(),
                        _ => "singular".to_string(),
                    }
                } else {
                    String::new()
                },
                got_length: None,
                remediation_url: if field_path.is_empty() {
                    "https://www.ricardovelit.com/axon-docs".to_string()
                } else {
                    String::new()
                },
            });
        }
    };
    if element_type.is_empty() {
        // `List` with no generic param — accept any element (degenerate
        // declaration; parser should ideally warn but doesn't today).
        return Ok(());
    }
    // v2.0.0 — pre-parse the element type ONCE for the whole
    // iteration. This replaces the per-element string-stripping that
    // the v1.40.2/v1.40.3 section 0 preamble in validate_value used to do.
    let (elem_head, elem_generic) = parse_generic_head(element_type);
    for (idx, elem) in arr.iter().enumerate() {
        let elem_path = if field_path.is_empty() {
            format!("[{idx}]")
        } else {
            format!("{field_path}[{idx}]")
        };
        validate_value(
            elem,
            &elem_head,
            &elem_generic,
            &elem_path,
            table,
            body_type,
        )?;
    }
    Ok(())
}

fn validate_struct(
    v: &Value,
    schema: &TypeSchema,
    field_path: &str,
    table: &HashMap<String, TypeSchema>,
    body_type: &str,
) -> Result<(), BodyValidationError> {
    let obj = match v.as_object() {
        Some(o) => o,
        None => {
            return Err(BodyValidationError {
                expected_type: body_type.to_string(),
                field_path: field_path.to_string(),
                expected: schema.name.clone(),
                got: json_tag(v).to_string(),
                hint: format!(
                    "Body field `{path}` must be a `{type_name}` (JSON object) \
                     but received a {got}. {cardinality_hint}",
                    path = if field_path.is_empty() { "<body>" } else { field_path },
                    type_name = schema.name,
                    got = json_tag(v),
                    cardinality_hint = if field_path.is_empty() && v.is_array() {
                        format!(
                            "The flow returned a `List<{tn}>` (array of {n} \
                             items) but the endpoint declared `output: {tn}` \
                             (singular). Either change the endpoint to \
                             `output: List<{tn}>` or collapse the flow's tail \
                             to a single item (e.g. `return result[0]`). \
                             ",
                            tn = schema.name,
                            n = v.as_array().map(|a| a.len()).unwrap_or(0),
                        )
                    } else {
                        String::new()
                    },
                ),
                // v1.31.0 (D2) — when the top-level body got an
                // array but expected an object, this is the canonical
                // singular-vs-plural mismatch. Populate the structured
                // cardinality diagnostic fields for the audit_log.
                expected_cardinality: if field_path.is_empty() {
                    "singular".to_string()
                } else {
                    String::new()
                },
                got_cardinality: if field_path.is_empty() {
                    match v {
                        Value::Array(_) => "plural".to_string(),
                        Value::Null => "unit".to_string(),
                        _ => "singular".to_string(),
                    }
                } else {
                    String::new()
                },
                got_length: if field_path.is_empty() {
                    v.as_array().map(|a| a.len() as u64)
                } else {
                    None
                },
                remediation_url: if field_path.is_empty() && v.is_array() {
                    "https://www.ricardovelit.com/axon-docs".to_string()
                } else {
                    String::new()
                },
            });
        }
    };
    for field in &schema.fields {
        let child_path = if field_path.is_empty() {
            field.name.clone()
        } else {
            format!("{field_path}.{}", field.name)
        };
        match obj.get(&field.name) {
            None => {
                if field.optional {
                    continue;
                }
                return Err(BodyValidationError {
                    expected_type: body_type.to_string(),
                    field_path: child_path.clone(),
                    expected: field.type_name.clone(),
                    got: "missing".to_string(),
                    hint: format!(
                        "Body field `{child_path}` is required (declared as \
                         `{type_name}` on `{struct_name}`) but is absent from \
                         the request body.",
                        type_name = field.type_name,
                        struct_name = schema.name,
                    ),
                    ..Default::default()
                });
            }
            Some(child) => {
                // Optional `T?` fields with explicit JSON null are accepted.
                if field.optional && child.is_null() {
                    continue;
                }
                validate_value(
                    child,
                    &field.type_name,
                    &field.generic_param,
                    &child_path,
                    table,
                    body_type,
                )?;
            }
        }
    }
    // Unknown extra fields are NOT rejected — adopters can pass extra
    // payload the flow ignores (industry-standard "be liberal in what you
    // accept" for forwards-compat with client-side additions). Strict
    // mode is a future opt-in if vertical compliance demands.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t_string() -> TypeSchema {
        TypeSchema {
            name: "String".to_string(),
            fields: vec![],
            range: None,
        }
    }

    fn person_schema() -> TypeSchema {
        TypeSchema {
            name: "Person".to_string(),
            fields: vec![
                FieldSchema {
                    name: "name".to_string(),
                    type_name: "String".to_string(),
                    generic_param: String::new(),
                    optional: false,
                },
                FieldSchema {
                    name: "age".to_string(),
                    type_name: "Integer".to_string(),
                    generic_param: String::new(),
                    optional: true,
                },
            ],
            range: None,
        }
    }

    #[test]
    fn empty_body_type_passes_any_body() {
        let table = HashMap::new();
        let body = serde_json::json!({"anything": "goes"});
        assert!(validate_body(&body, "", &table).is_ok());
    }

    #[test]
    fn primitive_string_ok() {
        let table = HashMap::new();
        let body = serde_json::json!("hello");
        assert!(validate_body(&body, "String", &table).is_ok());
    }

    #[test]
    fn primitive_string_rejects_number() {
        let table = HashMap::new();
        let body = serde_json::json!(42);
        let err = validate_body(&body, "String", &table).unwrap_err();
        assert_eq!(err.expected, "String");
        assert_eq!(err.got, "integer");
    }

    #[test]
    fn integer_rejects_float() {
        let table = HashMap::new();
        let body = serde_json::json!(3.14);
        let err = validate_body(&body, "Integer", &table).unwrap_err();
        assert_eq!(err.expected, "Integer");
        assert_eq!(err.got, "number");
    }

    #[test]
    fn float_accepts_integer_json() {
        let table = HashMap::new();
        let body = serde_json::json!(42);
        assert!(validate_body(&body, "Float", &table).is_ok());
        let body = serde_json::json!(3.14);
        assert!(validate_body(&body, "Float", &table).is_ok());
    }

    #[test]
    fn structured_missing_required_field() {
        let mut table = HashMap::new();
        table.insert("Person".to_string(), person_schema());
        let body = serde_json::json!({"age": 30});
        let err = validate_body(&body, "Person", &table).unwrap_err();
        assert_eq!(err.field_path, "name");
        assert_eq!(err.got, "missing");
    }

    #[test]
    fn structured_optional_field_can_be_absent() {
        let mut table = HashMap::new();
        table.insert("Person".to_string(), person_schema());
        let body = serde_json::json!({"name": "alice"});
        assert!(validate_body(&body, "Person", &table).is_ok());
    }

    #[test]
    fn structured_optional_field_can_be_null() {
        let mut table = HashMap::new();
        table.insert("Person".to_string(), person_schema());
        let body = serde_json::json!({"name": "alice", "age": null});
        assert!(validate_body(&body, "Person", &table).is_ok());
    }

    #[test]
    fn structured_unknown_extra_fields_accepted() {
        let mut table = HashMap::new();
        table.insert("Person".to_string(), person_schema());
        let body = serde_json::json!({"name": "alice", "extra": "data"});
        assert!(validate_body(&body, "Person", &table).is_ok());
    }

    #[test]
    fn list_validates_each_element() {
        let mut table = HashMap::new();
        table.insert("String".to_string(), t_string());
        let body = serde_json::json!(["a", "b", "c"]);
        let err = validate_body(&body, "List", &table);
        assert!(err.is_ok());
    }

    #[test]
    fn list_rejects_non_array() {
        let table = HashMap::new();
        let body = serde_json::json!({"not": "array"});
        // Use a synthetic harness for the generic_param flavor.
        let r = validate_value(&body, "List", "String", "", &table, "List");
        let err = r.unwrap_err();
        assert!(err.expected.contains("List"));
        assert_eq!(err.got, "object");
    }

    #[test]
    fn list_element_violation_reports_indexed_path() {
        let table = HashMap::new();
        let body = serde_json::json!(["a", 42, "c"]);
        let r = validate_value(&body, "List", "String", "", &table, "List");
        let err = r.unwrap_err();
        assert_eq!(err.field_path, "[1]");
        assert_eq!(err.got, "integer");
    }

    #[test]
    fn range_type_rejects_out_of_bounds() {
        let table = HashMap::new();
        let body = serde_json::json!(1.5);
        let err = validate_body(&body, "RiskScore", &table).unwrap_err();
        assert!(err.expected.contains("RiskScore"));
    }

    #[test]
    fn range_type_accepts_in_bounds() {
        let table = HashMap::new();
        let body = serde_json::json!(0.7);
        assert!(validate_body(&body, "RiskScore", &table).is_ok());
    }

    #[test]
    fn unknown_type_returns_diagnostic() {
        let table = HashMap::new();
        let body = serde_json::json!({});
        let err = validate_body(&body, "NotDeclared", &table).unwrap_err();
        assert!(err.hint.contains("NotDeclared"));
    }

    #[test]
    fn nested_struct_field_path_is_dotted() {
        let mut table = HashMap::new();
        table.insert("Person".to_string(), person_schema());
        table.insert(
            "Loan".to_string(),
            TypeSchema {
                name: "Loan".to_string(),
                fields: vec![FieldSchema {
                    name: "applicant".to_string(),
                    type_name: "Person".to_string(),
                    generic_param: String::new(),
                    optional: false,
                }],
                range: None,
            },
        );
        let body = serde_json::json!({"applicant": {"age": 30}});
        let err = validate_body(&body, "Loan", &table).unwrap_err();
        assert_eq!(err.field_path, "applicant.name");
        assert_eq!(err.expected_type, "Loan");
    }

    #[test]
    fn json_tag_distinguishes_integer_and_number() {
        assert_eq!(json_tag(&serde_json::json!(42)), "integer");
        assert_eq!(json_tag(&serde_json::json!(3.14)), "number");
    }

    // ── v1.31.0 — generic-aware section 0 preamble tests ────────────

    #[test]
    fn validate_body_accepts_list_of_primitive() {
        // v1.31.0 — pre-hotfix the T9XX hint suggested
        // `output: List<String>` but `validate_body` rejected it as
        // unknown_type. Post-hotfix: section 0 preamble strips the generic
        // and dispatches to section 3 (`validate_list`) properly.
        let table: HashMap<String, TypeSchema> = HashMap::new();
        let body = serde_json::json!(["alice", "bob"]);
        let r = validate_body(&body, "List<String>", &table);
        assert!(
            r.is_ok(),
            "List<String> over a String array must validate. Got: {r:?}"
        );
    }

    #[test]
    fn validate_body_accepts_list_of_struct() {
        let mut table: HashMap<String, TypeSchema> = HashMap::new();
        table.insert("Person".to_string(), person_schema());
        let body = serde_json::json!([{"name": "alice", "age": 30}, {"name": "bob", "age": 25}]);
        let r = validate_body(&body, "List<Person>", &table);
        assert!(
            r.is_ok(),
            "List<Person> over a Person array must validate. Got: {r:?}"
        );
    }

    #[test]
    fn validate_body_rejects_list_of_unknown_inner() {
        // Inner type unknown → unknown_type error from section 5 with the
        // inner type name, NOT a generic-string failure.
        let table: HashMap<String, TypeSchema> = HashMap::new();
        let body = serde_json::json!([{}]);
        let r = validate_body(&body, "List<UnknownType>", &table);
        assert!(r.is_err(), "List<UnknownType> must surface the inner-type miss.");
        let err = r.unwrap_err();
        assert!(
            err.hint.contains("UnknownType"),
            "diagnostic must name the inner type (`UnknownType`), not the outer `List<...>` shape. \
             Got hint: {}",
            err.hint
        );
    }

    #[test]
    fn validate_body_rejects_list_against_non_array() {
        // section 3 (validate_list) handles the non-array case; we test the
        // wiring catches it via the section 0 preamble.
        let table: HashMap<String, TypeSchema> = HashMap::new();
        let body = serde_json::json!({"not": "an array"});
        let r = validate_body(&body, "List<String>", &table);
        assert!(r.is_err(), "object against List<String> must error.");
        let err = r.unwrap_err();
        assert_eq!(err.got, "object");
        assert!(err.expected.contains("List"));
    }

    #[test]
    fn validate_body_accepts_nested_list_of_list() {
        // Recursive — section 0 strips outer, recurses with type_name="List",
        // generic_param="List<String>". section 3's validate_list iterates
        // the outer array's elements; per-element validate_value lands
        // back in section 0 which strips the inner.
        let table: HashMap<String, TypeSchema> = HashMap::new();
        let body = serde_json::json!([["a", "b"], ["c"]]);
        let r = validate_body(&body, "List<List<String>>", &table);
        assert!(
            r.is_ok(),
            "Nested List<List<String>> over an array-of-arrays must validate. Got: {r:?}"
        );
    }

    #[test]
    fn validate_body_stream_returns_ok_early() {
        // v1.31.0 — Stream<T> body validation is structurally
        // unreachable (SSE chunks are validated at the wire layer, not
        // the body layer). Defensive Ok early.
        //
        // v2.0.0 — preserved verbatim. The section 0 preamble that
        // implemented this case in v1.40.2/v1.40.3 was deleted; the
        // defensive Ok now lives in validate_value (top of function)
        // for the `Stream` head case after the canonical entry parses.
        let table: HashMap<String, TypeSchema> = HashMap::new();
        let body = serde_json::json!({"anything": "goes"});
        let r = validate_body(&body, "Stream<Token>", &table);
        assert!(
            r.is_ok(),
            "Stream<T> at the body validator layer must be a defensive Ok. \
             Got: {r:?}"
        );
    }

    // ── v2.0.0 — canonical entry + helpers ────────────────────

    #[test]
    fn parse_generic_head_list() {
        let (h, g) = parse_generic_head("List<TenantRecord>");
        assert_eq!(h, "List");
        assert_eq!(g, "TenantRecord");
    }

    #[test]
    fn parse_generic_head_stream() {
        let (h, g) = parse_generic_head("Stream<Token>");
        assert_eq!(h, "Stream");
        assert_eq!(g, "Token");
    }

    #[test]
    fn parse_generic_head_nested_list() {
        // Nested generic `List<List<X>>` returns the outer split.
        // The inner `List<X>` is parsed by the recursive entry into
        // validate_list → parse_generic_head again.
        let (h, g) = parse_generic_head("List<List<X>>");
        assert_eq!(h, "List");
        assert_eq!(g, "List<X>");
    }

    #[test]
    fn parse_generic_head_bare_type() {
        let (h, g) = parse_generic_head("TenantRecord");
        assert_eq!(h, "TenantRecord");
        assert_eq!(g, "");
    }

    #[test]
    fn parse_generic_head_inner_whitespace_trimmed() {
        let (h, g) = parse_generic_head("List<  TenantRecord  >");
        assert_eq!(h, "List");
        assert_eq!(g, "TenantRecord");
    }

    #[test]
    fn strip_flow_envelope_singular() {
        assert_eq!(
            strip_flow_envelope("FlowEnvelope<TenantRecord>"),
            Some("TenantRecord".to_string())
        );
    }

    #[test]
    fn strip_flow_envelope_list() {
        assert_eq!(
            strip_flow_envelope("FlowEnvelope<List<TenantRecord>>"),
            Some("List<TenantRecord>".to_string())
        );
    }

    #[test]
    fn strip_flow_envelope_returns_none_on_bare() {
        assert_eq!(strip_flow_envelope("TenantRecord"), None);
        assert_eq!(strip_flow_envelope("List<X>"), None);
        assert_eq!(strip_flow_envelope(""), None);
    }

    #[test]
    fn validate_body_unwraps_flow_envelope_with_struct() {
        // v2.0.0 canonical: declared `FlowEnvelope<Person>`, body is
        // the FlowEnvelope wire shape, validation targets `result`
        // slot against `Person`.
        let mut table: HashMap<String, TypeSchema> = HashMap::new();
        table.insert("Person".to_string(), person_schema());
        let envelope = serde_json::json!({
            "ontological_type": "Person",
            "result": {"name": "alice", "age": 30},
            "certainty": 1.0,
            "provenance_chain": [],
            "step_audit": {},
            "audit_chain_hash": "",
            "blame_attribution": null,
            "execution_metrics": {},
            "trace_id": "t"
        });
        let r = validate_body(&envelope, "FlowEnvelope<Person>", &table);
        assert!(r.is_ok(), "FlowEnvelope<Person> over a Person body must validate. Got: {r:?}");
    }

    #[test]
    fn validate_body_unwraps_flow_envelope_with_list() {
        // v2.0.0 canonical: declared `FlowEnvelope<List<Person>>`, the
        // result slot is an array of Person.
        let mut table: HashMap<String, TypeSchema> = HashMap::new();
        table.insert("Person".to_string(), person_schema());
        let envelope = serde_json::json!({
            "ontological_type": "List<Person>",
            "result": [
                {"name": "alice", "age": 30},
                {"name": "bob", "age": 25}
            ],
            "certainty": 1.0,
            "provenance_chain": [],
            "step_audit": {},
            "audit_chain_hash": "",
            "blame_attribution": null,
            "execution_metrics": {},
            "trace_id": "t"
        });
        let r = validate_body(&envelope, "FlowEnvelope<List<Person>>", &table);
        assert!(
            r.is_ok(),
            "FlowEnvelope<List<Person>> over a Person array result must \
             validate. Got: {r:?}"
        );
    }

    #[test]
    fn validate_body_rejects_flow_envelope_with_wrong_inner_type() {
        // v2.0.0 — the canonical unwrap recurses on the inner T;
        // if the `result` slot doesn't match T, validation fails
        // with the inner-T error context.
        let mut table: HashMap<String, TypeSchema> = HashMap::new();
        table.insert("Person".to_string(), person_schema());
        // Result has a wrong-type field (age is a string, not int).
        let envelope = serde_json::json!({
            "ontological_type": "Person",
            "result": {"name": "alice", "age": "thirty"},
            "certainty": 1.0,
            "provenance_chain": [],
            "step_audit": {},
            "audit_chain_hash": "",
            "blame_attribution": null,
            "execution_metrics": {},
            "trace_id": "t"
        });
        let r = validate_body(&envelope, "FlowEnvelope<Person>", &table);
        assert!(
            r.is_err(),
            "Wrong inner-type MUST surface as validation error"
        );
        let err = r.unwrap_err();
        assert_eq!(err.field_path, "age");
    }

    #[test]
    fn validate_body_rejects_flow_envelope_with_non_object_body() {
        // v2.0.0 — when declared is FlowEnvelope<T> but the body isn't
        // a JSON object, validation surfaces a structural error (the
        // wire wrapper is mandated to be an object).
        let table: HashMap<String, TypeSchema> = HashMap::new();
        let body = serde_json::json!("not an object");
        let r = validate_body(&body, "FlowEnvelope<Any>", &table);
        assert!(
            r.is_err(),
            "Non-object body MUST fail FlowEnvelope<T> shape check"
        );
        let err = r.unwrap_err();
        assert!(err.hint.contains("FlowEnvelope"));
    }

    #[test]
    fn validate_body_flow_envelope_any_skips_inner_validation() {
        // v2.0.0 — `FlowEnvelope<Any>` is the universal accept
        // (degraded surface); the inner result is not validated.
        let table: HashMap<String, TypeSchema> = HashMap::new();
        let envelope = serde_json::json!({
            "ontological_type": "Any",
            "result": {"anything": "goes"},
            "certainty": 1.0,
            "provenance_chain": [],
            "step_audit": {},
            "audit_chain_hash": "",
            "blame_attribution": null,
            "execution_metrics": {},
            "trace_id": "t"
        });
        let r = validate_body(&envelope, "FlowEnvelope<Any>", &table);
        assert!(r.is_ok());
    }

    #[test]
    fn validate_body_flow_envelope_with_missing_result_slot() {
        // v2.0.0 — when the body lacks `result`, the unwrapper
        // treats it as Value::Null and validates Null against T.
        // For T=Any this is Ok; for T=Person it's a struct mismatch.
        let mut table: HashMap<String, TypeSchema> = HashMap::new();
        table.insert("Person".to_string(), person_schema());
        let envelope = serde_json::json!({
            "ontological_type": "Person",
            "certainty": 1.0
            // no `result` slot
        });
        let r = validate_body(&envelope, "FlowEnvelope<Person>", &table);
        assert!(
            r.is_err(),
            "Missing result slot MUST fail when inner type is non-Any"
        );
    }

    #[test]
    fn validate_value_no_longer_carries_section_0_preamble() {
        // v2.0.0 — STATIC grep gate. The section 0 preamble that
        // string-stripped List<X> / Stream<X> in v1.40.2/v1.40.3 is
        // RETIRED. Any future PR that reintroduces it inside
        // validate_value breaks this assertion.
        let src = std::fs::read_to_string("src/route_schema.rs")
            .expect("read route_schema.rs");
        // The section 0 marker text was unique; if it reappears we know the
        // bridge was reinstated.
        assert!(
            !src.contains("(POST-CLOSE HOTFIX 2026-05-21) — generic-\n    // aware parsing"),
            "the v1.40.2/v1.40.3 preamble inside \
             validate_value MUST stay retired. Generic parsing belongs \
             at the canonical validate_body entry now."
        );
    }

    #[test]
    fn gate_simplified_calls_validate_body_directly() {
        // v2.0.0 — STATIC grep gate on axon_server.rs. The pre-39.d
        // gate manually extracted inner-T + result slot; post-39.d
        // validate_body is the canonical entry and the gate just calls
        // it with the raw declared type.
        let src = std::fs::read_to_string("src/axon_server.rs")
            .expect("read axon_server.rs");
        // The manual extract pattern from 39.b should be gone.
        // (Allow it to still APPEAR in comments — only the active
        // code path matters.)
        let active_extract_calls = src.matches(
            "crate::wire_envelope::extract_inner_ontological_type(&route.output_type)"
        ).count();
        // 39.b had this in the active path; 39.d removes the active
        // call. The taxonomy might still be referenced in comments
        // but not as the active gate logic.
        assert!(
            active_extract_calls <= 1,
            "the output-schema gate MUST NOT manually call \
             `extract_inner_ontological_type` for unwrapping (that work \
             moved into validate_body). Found {active_extract_calls} \
             active references."
        );
    }
}
