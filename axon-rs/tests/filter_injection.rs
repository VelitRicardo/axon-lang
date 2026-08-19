//! v1.32.0 (D3) — injection-safe `${name}` resolution in a store
//! `where:` clause.
//!
//! The Request Binding Contract (37.b) puts request-body values in the
//! flow's interpolation scope. D3 closes the question those values
//! raise the moment they reach a store filter: a `${name}` in a
//! `where:` clause MUST become a `$N` bind parameter — never text
//! spliced into the `where` source before parsing.
//!
//! The mechanism (`store::filter`): the `where` expression is
//! tokenized FIRST (raw), so every string-literal boundary is fixed
//! before any value is substituted; THEN each `Token::Str`'s content
//! is interpolated against the bindings. A resolved value therefore
//! lives only inside an already-delimited string token — it is
//! rendered as a `$N` placeholder, and a value carrying `'`, `;`,
//! `--`, or `OR '1'='1'` cannot move a literal boundary or inject
//! filter syntax. Injection (OWASP A03) is closed by construction.
//!
//! This pack proves it PURELY on `build_pg_where` — no database, no
//! runtime, fully deterministic. The function is the single chokepoint
//! every Postgres `retrieve`/`mutate`/`purge` `where:` clause flows
//! through, on both the streaming and the synchronous path.

use axon::store::filter::{build_pg_where, SqlValue};
use std::collections::HashMap;

fn binds(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn txt(s: &str) -> SqlValue {
    SqlValue::Text(s.to_string())
}

// ── section 1 — a `${name}` resolves to a `$N` bind parameter ──────────────

#[test]
fn s1_placeholder_resolves_to_a_bind_parameter() {
    let b = binds(&[("tenant_id", "acme-corp")]);
    let (clause, params) =
        build_pg_where("id = '${tenant_id}'", 0, &b, &HashMap::new())
            .expect("compiles");
    assert_eq!(
        clause, "\"id\"::text = $1",
        "v1.32.0 D3 — the `${{tenant_id}}` value is a $N placeholder, \
         not spliced into the clause (no column type supplied, so \
         v1.32.0 D4 casts the column to `text` for the equality)"
    );
    assert_eq!(
        params,
        vec![txt("acme-corp")],
        "v1.32.0 D3 — the resolved value travels as a bind parameter"
    );
}

// ── section 2 — a SQL-injection payload is an inert bind parameter ─────────

#[test]
fn s2_sql_injection_payload_is_an_inert_bind_parameter() {
    // The classic: a request value crafted to drop a table.
    let b = binds(&[("x", "'; DROP TABLE users; --")]);
    let (clause, params) =
        build_pg_where("name = '${x}'", 0, &b, &HashMap::new()).expect("compiles");
    assert_eq!(
        clause, "\"name\"::text = $1",
        "v1.32.0 D3 — the clause is EXACTLY one bound condition; the \
         payload did not become SQL. Clause: {clause}"
    );
    assert!(
        !clause.to_uppercase().contains("DROP"),
        "v1.32.0 D3 — `DROP` must NOT appear in the SQL clause — it is \
         inside the bind parameter. Clause: {clause}"
    );
    assert_eq!(
        params,
        vec![txt("'; DROP TABLE users; --")],
        "v1.32.0 D3 — the entire payload is one inert bind value"
    );
}

// ── section 3 — a filter-logic injection (`OR '1'='1'`) cannot fire ────────

#[test]
fn s3_filter_logic_injection_cannot_add_a_condition() {
    // A value crafted to turn `id = '<x>'` into `id = '' OR '1'='1'`.
    let b = binds(&[("x", "' OR '1'='1")]);
    let (clause, params) =
        build_pg_where("id = '${x}'", 0, &b, &HashMap::new()).expect("compiles");
    assert_eq!(
        clause, "\"id\"::text = $1",
        "v1.32.0 D3 — the clause stays a SINGLE condition; the injected \
         `OR` did not become a connector. Clause: {clause}"
    );
    assert!(
        !clause.to_uppercase().contains(" OR "),
        "v1.32.0 D3 — no `OR` connector was injected into the clause"
    );
    assert_eq!(params, vec![txt("' OR '1'='1")]);
}

// ── section 4 — the boundary theorem: a `'` cannot escape the literal ──────

#[test]
fn s4_a_quote_in_the_value_cannot_escape_the_literal() {
    // The where-STRING is tokenized BEFORE substitution, so the
    // literal boundary is fixed — a `'` in the value is just data.
    let b = binds(&[("x", "boundary' AND \"secret\" = 'leaked")]);
    let (clause, params) =
        build_pg_where("col = '${x}'", 0, &b, &HashMap::new()).expect("compiles");
    assert_eq!(
        clause, "\"col\"::text = $1",
        "v1.32.0 D3 — exactly one condition; the value's `'` did not \
         re-open the grammar. Clause: {clause}"
    );
    assert!(
        !clause.contains("secret"),
        "v1.32.0 D3 — the smuggled `\"secret\"` column never reached \
         the SQL clause. Clause: {clause}"
    );
    assert_eq!(params, vec![txt("boundary' AND \"secret\" = 'leaked")]);
}

// ── section 5 — a `${name}` embedded in a LIKE pattern ─────────────────────

#[test]
fn s5_placeholder_embedded_in_a_like_pattern() {
    let b = binds(&[("q", " admin")]);
    let (clause, params) =
        build_pg_where("name LIKE '%${q}%'", 0, &b, &HashMap::new()).expect("compiles");
    assert_eq!(clause, "\"name\" LIKE $1");
    assert_eq!(
        params,
        vec![txt("% admin%")],
        "v1.32.0 D3 — `${{q}}` interpolates INSIDE the string literal; \
         the whole pattern is one bind value"
    );
}

// ── section 6 — multiple placeholders bind in order ────────────────────────

#[test]
fn s6_multiple_placeholders_bind_in_order() {
    let b = binds(&[("a", "first"), ("c", "third")]);
    let (clause, params) =
        build_pg_where("x = '${a}' AND y = '${c}'", 0, &b, &HashMap::new()).expect("compiles");
    assert_eq!(clause, "\"x\"::text = $1 AND \"y\"::text = $2");
    assert_eq!(params, vec![txt("first"), txt("third")]);
}

// ── section 7 — an unbound `${name}` stays literal (no injection) ──────────

#[test]
fn s7_unbound_placeholder_stays_literal_and_inert() {
    // An unknown variable is left literal (the `interpolate_vars`
    // contract) — a wrong value, but a bound, inert one. Never a
    // compile error, never a splice.
    let (clause, params) =
        build_pg_where("id = '${missing}'", 0, &HashMap::new(), &HashMap::new()).expect("compiles");
    assert_eq!(clause, "\"id\"::text = $1");
    assert_eq!(
        params,
        vec![txt("${missing}")],
        "v1.32.0 — an unbound placeholder binds the literal token as an \
         inert text parameter (matches 0 rows; never injects)"
    );
}

// ── section 8 — D5: an empty bindings map is byte-identical to pre-37.d ────

#[test]
fn s8_d5_empty_bindings_is_backwards_compatible() {
    // A literal `where` clause with no `${...}` compiles identically
    // whether or not a bindings map is supplied.
    let (clause, params) =
        build_pg_where(
            "id = 'literal' AND n = 7",
            0,
            &HashMap::new(),
            &HashMap::new(),
        )
        .expect("compiles");
    assert_eq!(clause, "\"id\"::text = $1 AND \"n\"::text = $2");
    assert_eq!(params, vec![txt("literal"), SqlValue::Integer(7)]);
}

// ── section 9 — the `$name` (brace-less) form also resolves ────────────────

#[test]
fn s9_braceless_dollar_form_resolves() {
    let b = binds(&[("session", "sess-42")]);
    let (clause, params) =
        build_pg_where("sid = '$session'", 0, &b, &HashMap::new()).expect("compiles");
    assert_eq!(clause, "\"sid\"::text = $1");
    assert_eq!(params, vec![txt("sess-42")]);
}
