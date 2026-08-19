//! v1.32.0 (D3, D7) — property/fuzz pass over the Request Binding
//! Contract's two runtime chokepoints.
//!
//! Both surfaces are PURE functions — fuzzed deterministically (a
//! hand-rolled LCG, no external dep) with the invariants asserted on
//! every generated input:
//!
//!   SURFACE A — `request_binding::bind_request_body` (D1, D4).
//!     Over arbitrary `(flow parameters, request body)` shapes:
//!       - total — never panics, for any body shape (object, scalar,
//!         array, null, absent);
//!       - D4 — every bound name is a DECLARED flow parameter that is
//!         also a body field; an undeclared body field never binds;
//!       - deterministic — the same input yields the same output;
//!       - order — the binding follows parameter declaration order.
//!
//!   SURFACE B — `store::filter::build_pg_where` with `${name}`
//!     resolution (D3 — injection resistance). Over arbitrary
//!     `where` templates × adversarial binding values (SQL
//!     metacharacters, filter syntax, quotes, nested `${...}`):
//!       - total — never panics;
//!       - STRUCTURE IS TEMPLATE-DETERMINED — a K-condition template
//!         compiles to exactly K `$N` placeholders + K bind params,
//!         regardless of the values; an adversarial value can NOT add
//!         a condition;
//!       - NO VALUE LEAK — no resolved value's text appears in the
//!         rendered SQL clause; it lives only in the bind-parameter
//!         vector. Injection is closed by construction.

use axon::ir_nodes::{IRFlow, IRParameter};
use axon::store::filter::{build_pg_where, SqlValue};
use std::collections::HashMap;

// ── Deterministic LCG (reproducible from the seed) ──────────────────

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn n(&mut self, m: u64) -> u64 {
        if m == 0 {
            0
        } else {
            self.next() % m
        }
    }
}

// ════════════════════════════════════════════════════════════════════
//  SURFACE A — bind_request_body (D1, D4)
// ════════════════════════════════════════════════════════════════════

/// A name pool small enough that flow parameters and body fields
/// collide often — exercising both the bind path and the D4
/// "undeclared field is ignored" path.
const NAMES: &[&str] = &["message", "tenant_id", "session_id", "topic", "x", "y"];
const TYPES: &[&str] = &["String", "Int", "Float", "Bool"];

fn param(name: &str, type_name: &str, optional: bool) -> IRParameter {
    IRParameter {
        node_type: "parameter",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        type_name: type_name.into(),
        generic_param: String::new(),
        optional,
    }
}

fn flow_with(params: Vec<IRParameter>) -> IRFlow {
    IRFlow {
        node_type: "flow",
        source_line: 0,
        source_column: 0,
        name: "F".into(),
        parameters: params,
        return_type_name: "Unit".into(),
        return_type_generic: String::new(),
        return_type_optional: false,
        steps: Vec::new(),
        edges: Vec::new(),
        execution_levels: Vec::new(),
    }
}

/// Generate a flow with 0..=5 parameters, UNIQUE names (a real flow
/// cannot declare a name twice — the type-checker forbids it).
fn gen_flow(lcg: &mut Lcg) -> IRFlow {
    let count = lcg.n(6) as usize;
    let mut used = Vec::new();
    let mut params = Vec::new();
    for _ in 0..count {
        let name = NAMES[lcg.n(NAMES.len() as u64) as usize];
        if used.contains(&name) {
            continue;
        }
        used.push(name);
        params.push(param(
            name,
            TYPES[lcg.n(TYPES.len() as u64) as usize],
            lcg.n(4) == 0,
        ));
    }
    flow_with(params)
}

/// Generate an arbitrary JSON value — scalar, null, nested, array,
/// including adversarial strings.
fn gen_value(lcg: &mut Lcg) -> serde_json::Value {
    match lcg.n(8) {
        0 => serde_json::json!("plain-value"),
        1 => serde_json::json!("'; DROP TABLE x; --"),
        2 => serde_json::json!(lcg.next() as i64),
        3 => serde_json::json!(lcg.n(2) == 0),
        4 => serde_json::Value::Null,
        5 => serde_json::json!({ "nested": "object" }),
        6 => serde_json::json!([1, 2, 3]),
        _ => serde_json::json!("${re_interpolation_token}"),
    }
}

/// Generate an arbitrary request body — usually an object, sometimes
/// a bare scalar / array / absent (the non-object paths).
fn gen_body(lcg: &mut Lcg) -> Option<serde_json::Value> {
    match lcg.n(10) {
        0 => None,
        1 => Some(serde_json::json!("bare-scalar")),
        2 => Some(serde_json::json!([1, 2])),
        _ => {
            let count = lcg.n(7) as usize;
            let mut map = serde_json::Map::new();
            for _ in 0..count {
                let name = NAMES[lcg.n(NAMES.len() as u64) as usize];
                map.insert(name.to_string(), gen_value(lcg));
            }
            Some(serde_json::Value::Object(map))
        }
    }
}

#[tokio::test]
async fn surface_a_bind_request_body_is_total_and_d4_correct() {
    let mut lcg = Lcg(0x3737_6700_4269_6E64);
    for iter in 0..2000u64 {
        let flow = gen_flow(&mut lcg);
        let body = gen_body(&mut lcg);

        // Total — the call returning is the never-panic assertion.
        let bound = axon::request_binding::bind_request_body(&flow, body.as_ref());

        // ── D4 — every bound name is a DECLARED parameter that is
        //    also present as a body field. An undeclared body field
        //    never appears; a parameter absent from the body never
        //    appears.
        let body_obj = match body.as_ref() {
            Some(serde_json::Value::Object(m)) => Some(m),
            _ => None,
        };
        for (name, _) in &bound {
            assert!(
                flow.parameters.iter().any(|p| &p.name == name),
                "v1.32.0 D4 — iter {iter}: bound name `{name}` is not a \
                 declared flow parameter"
            );
            assert!(
                body_obj.is_some_and(|m| m.contains_key(name)),
                "v1.32.0 D4 — iter {iter}: bound name `{name}` is not a \
                 field of the request body"
            );
        }
        // The bound set is exactly {declared params} ∩ {body fields}.
        let expected: Vec<&str> = flow
            .parameters
            .iter()
            .filter(|p| body_obj.is_some_and(|m| m.contains_key(&p.name)))
            .map(|p| p.name.as_str())
            .collect();
        let got: Vec<&str> = bound.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            got, expected,
            "v1.32.0 D1/D4 — iter {iter}: the binding must be exactly \
             the declared parameters present in the body, IN \
             declaration order"
        );

        // ── Deterministic — same input, same output.
        let again = axon::request_binding::bind_request_body(&flow, body.as_ref());
        assert_eq!(
            bound, again,
            "v1.32.0 — iter {iter}: bind_request_body must be deterministic"
        );
    }
}

// ════════════════════════════════════════════════════════════════════
//  SURFACE B — build_pg_where injection resistance (D3)
// ════════════════════════════════════════════════════════════════════

const COLS: &[&str] = &["id", "tenant", "status", "name"];
const OPS: &[&str] = &["=", "!=", "LIKE"];

/// Adversarial binding values — every one is crafted to inject SQL,
/// filter logic, or a re-interpolation if it were string-spliced.
/// Each is long + distinctive so a coincidental clause substring is
/// impossible.
const ADVERSARIAL: &[&str] = &[
    "'; DROP TABLE users; --",
    "x' OR '1'='1",
    "a' AND \"secret\" = 'leak",
    "${nested_token_should_not_expand}",
    "plain-but-tracked-value-42",
    "%wild' UNION SELECT * FROM pg_user --",
    "\\'; DELETE FROM sessions; --",
];

/// Generate a `where` template of 1..=4 conditions, each `col OP
/// '${vN}'`, joined by AND or OR — plus the bindings (one adversarial
/// value per placeholder).
fn gen_where(lcg: &mut Lcg) -> (String, HashMap<String, String>, usize, Vec<String>) {
    let k = 1 + lcg.n(4) as usize;
    let mut conds = Vec::new();
    let mut bindings = HashMap::new();
    let mut values = Vec::new();
    for i in 0..k {
        let col = COLS[lcg.n(COLS.len() as u64) as usize];
        let op = OPS[lcg.n(OPS.len() as u64) as usize];
        let var = format!("v{i}");
        let value = ADVERSARIAL[lcg.n(ADVERSARIAL.len() as u64) as usize].to_string();
        bindings.insert(var.clone(), value.clone());
        values.push(value);
        conds.push(format!("{col} {op} '${{{var}}}'"));
    }
    let joiner = if lcg.n(2) == 0 { " AND " } else { " OR " };
    (conds.join(joiner), bindings, k, values)
}

#[test]
fn surface_b_build_pg_where_is_total_and_injection_resistant() {
    let mut lcg = Lcg(0x3737_6700_5371_6C57);
    for iter in 0..2000u64 {
        let (template, bindings, k, values) = gen_where(&mut lcg);

        // Total — never panics for any template × adversarial values.
        // v1.36.4 — empty `column_types`: the structural invariants
        // below (K params, K `$N`, no value leak) hold regardless of
        // the cast suffix.
        let result = build_pg_where(&template, 0, &bindings, &HashMap::new());

        // Every generated template is well-formed → always compiles.
        let (clause, params) = result.unwrap_or_else(|e| {
            panic!("v1.32.0 D3 — iter {iter}: a well-formed template must \
                    compile; got {e:?}. template: {template}")
        });

        // ── STRUCTURE IS TEMPLATE-DETERMINED — K conditions yield
        //    exactly K bind parameters + K `$N` placeholders, no
        //    matter what the adversarial values contain. An injected
        //    `OR`/`;`/quote can NOT add a condition.
        assert_eq!(
            params.len(),
            k,
            "v1.32.0 D3 — iter {iter}: a {k}-condition template must \
             bind exactly {k} parameters — an adversarial value did \
             not add one. clause: {clause}"
        );
        assert_eq!(
            clause.matches('$').count(),
            k,
            "v1.32.0 D3 — iter {iter}: the clause must carry exactly {k} \
             `$N` placeholders. clause: {clause}"
        );

        // ── NO VALUE LEAK — no resolved value's text appears in the
        //    rendered SQL clause; every value lives ONLY in the bind
        //    parameter vector. This is injection closed by
        //    construction.
        for value in &values {
            assert!(
                !clause.contains(value.as_str()),
                "v1.32.0 D3 — iter {iter}: a request value leaked into \
                 the SQL clause text — it must be a $N bind parameter \
                 only. value: {value:?}  clause: {clause}"
            );
        }
        // Every bind parameter is a Text value (the binding map is
        // string-typed) — and it carries the value verbatim, inert.
        for p in &params {
            assert!(
                matches!(p, SqlValue::Text(_)),
                "v1.32.0 D3 — iter {iter}: a resolved placeholder binds \
                 as Text. got: {p:?}"
            );
        }
    }
}

#[test]
fn surface_b_unbound_and_empty_bindings_never_panic() {
    // An unbound `${name}` stays literal (inert); an empty map is the
    // pre-37.d behaviour. Neither path may panic.
    let mut lcg = Lcg(0x3737_6700_456D_7074);
    for _ in 0..500u64 {
        let (template, _, _, _) = gen_where(&mut lcg);
        let _ = build_pg_where(
            &template,
            lcg.n(32) as usize,
            &HashMap::new(),
            &HashMap::new(),
        );
    }
}
