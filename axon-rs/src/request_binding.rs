//! v1.32.0 — The Request Binding Contract (runtime delivery).
//!
//! An `axonendpoint` declares `body: T` (the typed request body) and
//! `execute: F` (a flow with declared parameters). The contract: the
//! request body's fields populate F's parameters — BY NAME (D1).
//!
//! v1.38.5 extends the binding-source set: path
//! placeholders (`/api/users/{id}`) and query params declared via
//! `query: { name: Type? }` join the body as canonical binding
//! sources. The compile-time D3 + D4 check (extending v1.32.0 D2)
//! guarantees every flow parameter resolves to EXACTLY ONE source —
//! collisions are `axon-T901` compile errors — so the runtime merge
//! order is semantically irrelevant by construction.
//!
//! Only DECLARED flow parameters bind (D4): a body field that matches
//! no parameter is NOT silently injected into the interpolation
//! scope, so the compile-time totality check (37.c / D2) stays the
//! single gate on what a `${x}` can resolve to — a typo'd `${tenat}`
//! is a missing binding, never a silently-empty surprise.
//!
//! This module is the runtime delivery, consumed by BOTH execution
//! paths — the streaming dispatcher (`DispatchCtx.let_bindings`) and
//! the synchronous runner (`ExecContext`) — so an `axonendpoint`'s
//! `transport: sse` and `transport: json` routes bind identically.

use std::collections::HashMap;

use crate::ir_nodes::IRFlow;

/// v1.32.0 — Bind a request to a flow's declared parameters across
/// THREE binding sources: path placeholders (URL captures), query
/// string params, and a parsed JSON body.
///
/// For each parameter `p` of `flow`, the binder searches the three
/// maps in declaration-source precedence (D4 guarantees there is
/// AT MOST ONE source via compile-time `axon-T901`):
///
///  1. `path` — `HashMap<String, String>` (URL path placeholder
///     captures; values are URL-decoded raw text per HTTP convention).
///  2. `query` — `HashMap<String, String>` (URL query string; the
///     adopter passes the first value for multi-value keys per
///     v1.38.5 honest-scope semantics).
///  3. `body` — `Option<&Value>` (the parsed JSON body; the v1.36.0
///     surface, unchanged).
///
/// The result is ordered by the flow's parameter declaration order —
/// deterministic for tests and the 37.g property pass.
///
/// Empty `path` + empty `query` + `None` body is a no-op (D5
/// backwards-compat: callers that didn't pass path/query before
/// v1.38.5 use `bind_request_body` which delegates here with empty
/// maps; the result is byte-identical to the pre-37.y behavior).
pub fn bind_request(
    flow: &IRFlow,
    path: &HashMap<String, String>,
    query: &HashMap<String, String>,
    body: Option<&serde_json::Value>,
) -> Vec<(String, String)> {
    let body_fields: Option<&serde_json::Map<String, serde_json::Value>> = match body {
        Some(serde_json::Value::Object(m)) => Some(m),
        _ => None,
    };

    flow.parameters
        .iter()
        .filter_map(|param| {
            // Source precedence (D4 invariant — by construction the
            // value is in AT MOST one source; the lookup order is
            // documentation, not semantics). Path values are already
            // text; query values are already text; body values
            // stringify per `binding_string`.
            if let Some(v) = path.get(&param.name) {
                return Some((param.name.clone(), v.clone()));
            }
            if let Some(v) = query.get(&param.name) {
                return Some((param.name.clone(), v.clone()));
            }
            if let Some(fields) = body_fields {
                if let Some(value) = fields.get(&param.name) {
                    return Some((param.name.clone(), binding_string(value)));
                }
            }
            None
        })
        .collect()
}

/// v1.32.0 — Legacy body-only binder. Delegates to [`bind_request`]
/// with empty path + empty query maps. Preserved for source
/// backwards-compat with v1.36.0-style callers (test code,
/// non-axon-server programmatic consumers); D5 absolute guarantees
/// the return is byte-identical to the v1.36.0 implementation when
/// path + query are empty.
///
/// `body` is `None` (or a non-object JSON value) for a request with
/// no body, or a body that is a bare scalar / array — in every such
/// case the binding is empty and the flow runs with whatever bindings
/// its own `let` statements and step outputs produce.
pub fn bind_request_body(
    flow: &IRFlow,
    body: Option<&serde_json::Value>,
) -> Vec<(String, String)> {
    bind_request(flow, &HashMap::new(), &HashMap::new(), body)
}

/// Stringify a JSON value for the `String`-valued interpolation map
/// (`${name}` substitution is textual).
///
/// A JSON string binds to its raw contents (no surrounding quotes —
/// the value, not its JSON literal); `null` binds to the empty
/// string; a number / boolean binds to its canonical JSON form
/// (`42`, `true`). An array / object binds to its compact JSON form —
/// a structured parameter is honest future scope (the 37.c totality
/// check names a structured parameter explicitly rather than this
/// path binding it silently), but binding the compact JSON keeps the
/// function total and panic-free over every parsed body.
fn binding_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir_nodes::{IRFlow, IRParameter};

    fn param(name: &str) -> IRParameter {
        IRParameter {
            node_type: "parameter",
            source_line: 0,
            source_column: 0,
            name: name.into(),
            type_name: "String".into(),
            generic_param: String::new(),
            optional: false,
        }
    }

    fn flow_with_params(names: &[&str]) -> IRFlow {
        IRFlow {
            node_type: "flow",
            source_line: 0,
            source_column: 0,
            name: "F".into(),
            parameters: names.iter().map(|n| param(n)).collect(),
            return_type_name: "Unit".into(),
            return_type_generic: String::new(),
            return_type_optional: false,
            steps: Vec::new(),
            edges: Vec::new(),
            execution_levels: Vec::new(),
        }
    }

    #[test]
    fn binds_each_declared_parameter_by_name() {
        let flow = flow_with_params(&["message", "tenant_id"]);
        let body = serde_json::json!({
            "message": "hello",
            "tenant_id": "83d078e1-b372-42ba-9572-ff8dc521386e",
        });
        let bound = bind_request_body(&flow, Some(&body));
        assert_eq!(
            bound,
            vec![
                ("message".into(), "hello".into()),
                (
                    "tenant_id".into(),
                    "83d078e1-b372-42ba-9572-ff8dc521386e".into()
                ),
            ],
            "D1 — each declared parameter binds from its same-named body field"
        );
    }

    #[test]
    fn d4_an_undeclared_body_field_is_not_bound() {
        let flow = flow_with_params(&["message"]);
        let body = serde_json::json!({ "message": "hi", "extra": "ignored" });
        let bound = bind_request_body(&flow, Some(&body));
        assert_eq!(
            bound,
            vec![("message".into(), "hi".into())],
            "D4 — a body field with no matching declared parameter is \
             NOT bound; the contract stays tight"
        );
    }

    #[test]
    fn an_uncovered_parameter_simply_does_not_bind() {
        // D2 (37.c) makes this a compile error; at runtime the binding
        // is just absent — never a panic.
        let flow = flow_with_params(&["message", "session_id"]);
        let body = serde_json::json!({ "message": "hi" });
        let bound = bind_request_body(&flow, Some(&body));
        assert_eq!(bound, vec![("message".into(), "hi".into())]);
    }

    #[test]
    fn scalar_values_bind_as_their_string_form() {
        let flow = flow_with_params(&["s", "n", "b", "z"]);
        let body = serde_json::json!({
            "s": "raw", "n": 42, "b": true, "z": null,
        });
        let bound = bind_request_body(&flow, Some(&body));
        assert_eq!(
            bound,
            vec![
                ("s".into(), "raw".into()),   // string: no quotes
                ("n".into(), "42".into()),    // number: canonical
                ("b".into(), "true".into()),  // bool: canonical
                ("z".into(), String::new()),  // null: empty
            ]
        );
    }

    #[test]
    fn no_body_or_non_object_body_binds_nothing() {
        let flow = flow_with_params(&["message"]);
        assert!(bind_request_body(&flow, None).is_empty());
        assert!(bind_request_body(&flow, Some(&serde_json::json!("bare"))).is_empty());
        assert!(bind_request_body(&flow, Some(&serde_json::json!([1, 2]))).is_empty());
    }

    #[test]
    fn a_flow_with_no_parameters_binds_nothing() {
        let flow = flow_with_params(&[]);
        let body = serde_json::json!({ "message": "hi" });
        assert!(
            bind_request_body(&flow, Some(&body)).is_empty(),
            "D5 — a parameter-less flow is unaffected by any body"
        );
    }

    // ═══════════════════════════════════════════════════════════════
    // v1.32.0 — new 3-source `bind_request` tests
    // ═══════════════════════════════════════════════════════════════

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn d3_path_only_binding() {
        let flow = flow_with_params(&["tenant_id", "secret_name"]);
        let path = map(&[
            ("tenant_id", "acme"),
            ("secret_name", "api-key"),
        ]);
        let bound = bind_request(&flow, &path, &HashMap::new(), None);
        assert_eq!(
            bound,
            vec![
                ("tenant_id".into(), "acme".into()),
                ("secret_name".into(), "api-key".into()),
            ]
        );
    }

    #[test]
    fn d3_query_only_binding() {
        let flow = flow_with_params(&["status", "limit"]);
        let query = map(&[("status", "active"), ("limit", "50")]);
        let bound = bind_request(&flow, &HashMap::new(), &query, None);
        assert_eq!(
            bound,
            vec![
                ("status".into(), "active".into()),
                ("limit".into(), "50".into()),
            ]
        );
    }

    #[test]
    fn d3_mixed_path_query_body() {
        let flow = flow_with_params(&["tenant_id", "dry_run", "value"]);
        let path = map(&[("tenant_id", "acme")]);
        let query = map(&[("dry_run", "true")]);
        let body = serde_json::json!({ "value": "secret-payload" });
        let bound = bind_request(&flow, &path, &query, Some(&body));
        assert_eq!(
            bound,
            vec![
                ("tenant_id".into(), "acme".into()),
                ("dry_run".into(), "true".into()),
                ("value".into(), "secret-payload".into()),
            ],
            "D3 — each param resolves from its single declared source; \
             order follows the flow parameter declaration order"
        );
    }

    #[test]
    fn d4_invariant_value_taken_from_earliest_source_in_precedence() {
        // The compile-time D4 check makes multi-source declaration a
        // build error (axon-T901). At runtime, even if a caller
        // accidentally provided overlapping maps, the binder picks
        // path > query > body. This test documents the order; in
        // practice the maps cannot overlap by construction.
        let flow = flow_with_params(&["id"]);
        let path = map(&[("id", "from-path")]);
        let query = map(&[("id", "from-query")]);
        let body = serde_json::json!({ "id": "from-body" });
        let bound = bind_request(&flow, &path, &query, Some(&body));
        assert_eq!(bound, vec![("id".into(), "from-path".into())]);
    }

    #[test]
    fn d5_bind_request_body_legacy_delegate_byte_identical() {
        // The legacy `bind_request_body` MUST produce the exact same
        // result as the v1.36.0 implementation — empty path + empty
        // query maps means the new binder reduces to the old one.
        let flow = flow_with_params(&["message", "tenant_id"]);
        let body = serde_json::json!({
            "message": "hi",
            "tenant_id": "acme",
        });
        let via_legacy = bind_request_body(&flow, Some(&body));
        let via_new = bind_request(
            &flow,
            &HashMap::new(),
            &HashMap::new(),
            Some(&body),
        );
        assert_eq!(via_legacy, via_new, "D5 — legacy delegate is byte-identical");
    }

    #[test]
    fn d5_empty_inputs_yield_empty_binding() {
        let flow = flow_with_params(&["x", "y"]);
        let bound = bind_request(
            &flow,
            &HashMap::new(),
            &HashMap::new(),
            None,
        );
        assert!(bound.is_empty(), "D5 — empty everywhere ⇒ empty binding");
    }

    #[test]
    fn d4_undeclared_path_or_query_keys_are_ignored() {
        // A caller passing extra keys NOT in the flow signature: those
        // keys are silently ignored. Mirrors the body-side D4 invariant.
        let flow = flow_with_params(&["needed"]);
        let path = map(&[("needed", "v"), ("unrelated_path", "x")]);
        let query = map(&[("unrelated_query", "y")]);
        let bound = bind_request(&flow, &path, &query, None);
        assert_eq!(bound, vec![("needed".into(), "v".into())]);
    }

    #[test]
    fn kivi_end_to_end_runtime_binding() {
        // The kivi corpus at runtime: tenant_id + secret_name from
        // URL path captures, dry_run + overwrite from query, value
        // from body. Five declared flow params, three binding
        // sources, no collisions.
        let flow = flow_with_params(&[
            "tenant_id",
            "secret_name",
            "dry_run",
            "overwrite",
            "value",
        ]);
        let path = map(&[
            ("tenant_id", "acme-corp"),
            ("secret_name", "stripe-api-key"),
        ]);
        let query = map(&[
            ("dry_run", "true"),
            ("overwrite", "false"),
        ]);
        let body = serde_json::json!({
            "value": "sk_live_xxxxx",
        });
        let bound = bind_request(&flow, &path, &query, Some(&body));
        assert_eq!(
            bound,
            vec![
                ("tenant_id".into(), "acme-corp".into()),
                ("secret_name".into(), "stripe-api-key".into()),
                ("dry_run".into(), "true".into()),
                ("overwrite".into(), "false".into()),
                ("value".into(), "sk_live_xxxxx".into()),
            ]
        );
    }
}
