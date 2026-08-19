//! v2.67.0 — **the laws that make `resource` govern something.**
//!
//! # Why every one of these needs a refuted program
//!
//! v2.67.0's finding was that ~22 advertised primitives were declared, type-checked,
//! lowered into the IR — and then read by **nothing**. `resource` was the worst
//! of them: `capacity` and `lifetime` were parsed, lowered, and confirmed by
//! exhaustive grep to be read by **zero lines of runtime code in either repo**,
//! while the README sold `lifetime` as Linear Logic.
//!
//! So a test that merely says "this compiles" proves nothing here. The only
//! evidence a law exists is **a program it REFUSES**. Each test below writes the
//! violation and demands the specific diagnostic.
//!
//! | | |
//! |---|---|
//! | **axon-T942** | `kind:` is a closed catalog (it was an unvalidated free string) |
//! | **axon-T943** | `within:` names a declared `fabric` |
//! | **axon-T944** | `endpoint:` is a config key, never a URL literal (T850 conformance) |
//! | **axon-T945** | the Linear-Logic **sharing discipline** — how many holders may name it |
//! | **axon-T946** | a store names ONE source of truth (`resource:` XOR `connection:`) |
//! | **axon-T947** | manifest/fabric coherence |

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

/// Type-check `src`, returning every diagnostic as one joined string.
fn errors_of(src: &str) -> String {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&program)
        .check()
        .iter()
        .map(|e| e.message.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Type-check `src` and assert it is CLEAN.
fn accepts(src: &str) {
    let errs = errors_of(src);
    assert!(errs.is_empty(), "expected a clean program, got:\n{errs}");
}

/// Type-check `src` and assert `code` is among the diagnostics.
fn refutes(src: &str, code: &str) {
    let errs = errors_of(src);
    assert!(
        errs.contains(code),
        "expected {code} to REFUTE this program. A law with no refutation is not a law — \
         it is a comment. Diagnostics were:\n{errs}"
    );
}

/// The shape v2.67.0 is for. Every discipline the README advertises, attached to
/// the thing that actually runs.
const GOVERNED: &str = r#"
fabric   Prod { provider: aws  region: "us-east-1"  zones: 3 }
resource Db   { kind: postgres  within: Prod  endpoint: db.main
                lifetime: affine  capacity: 20 }
manifest Infra { fabric: Prod  resources: [Db] }
axonstore Users { backend: postgresql  resource: Db }
"#;

#[test]
fn the_governed_shape_compiles() {
    accepts(GOVERNED);
}

// ── axon-T942 — `kind:` is a closed catalog ──────────────────────────────────

/// Before v2.67.0 `resource.kind` was a **free string that nothing validated**. No
/// `VALID_RESOURCE_KINDS` const existed in the workspace and `check_resource`
/// never read the field — so this typo compiled clean and produced a resource
/// the runtime could never reach, silently.
#[test]
fn t942_a_misspelled_kind_is_refuted_it_used_to_compile_clean() {
    refutes(
        r#"resource Db { kind: postgress  endpoint: db.main }"#,
        "axon-T942",
    );
}

#[test]
fn t942_a_resource_with_no_kind_names_no_infrastructure() {
    refutes(r#"resource Db { endpoint: db.main }"#, "axon-T942");
}

// ── axon-T943 — `within:` names a declared fabric ────────────────────────────

/// `within:` is ONE field, so a resource cannot be in two fabrics: disjointness
/// is unrepresentable rather than verified. What still needs checking is that
/// the fabric EXISTS — a resource placed in a phantom fabric is placed nowhere,
/// and every Separation-Logic `*` claim about it is vacuous.
#[test]
fn t943_a_resource_within_a_phantom_fabric_is_placed_nowhere() {
    refutes(
        r#"resource Db { kind: postgres  endpoint: db.main  within: Ghost }"#,
        "axon-T943",
    );
}

// ── axon-T944 — the endpoint is a config key, never a URL ────────────────────

/// **The language already legislated this.** `axon-T850` refuses a URL literal
/// in `upstream.resolve` with the words *"URLs and credentials never appear in
/// source"*. `resource.endpoint` — the declaration that claims to be the single
/// source of truth for infrastructure — was a **grandfathered violation of that
/// same law**, and happily accepted a production DSN written into the program.
#[test]
fn t944_a_production_dsn_in_source_is_refuted_the_language_already_said_so() {
    refutes(
        r#"resource Db { kind: postgres  endpoint: "postgres://user:pw@prod-db/app" }"#,
        "axon-T944",
    );
}

// ── axon-T945 — the Linear-Logic sharing discipline ──────────────────────────

/// **`affine` = AT MOST ONE holder.** Sharing it is a breach.
///
/// This is not decoration. Today two axonstores silently share one connection
/// pool whenever their DSNs resolve equal — the registry caches pools keyed on
/// the resolved DSN. Nobody declared that sharing, nobody checked it, and
/// nothing tells you it happened. v2.67.0 makes it declared, and this law makes the
/// undeclared case refuse.
#[test]
fn t945_sharing_an_affine_resource_is_a_breach() {
    refutes(
        r#"
resource Db { kind: postgres  endpoint: db.main  lifetime: affine }
axonstore A { backend: postgresql  resource: Db }
axonstore B { backend: postgresql  resource: Db }
"#,
        "axon-T945",
    );
}

/// **`linear` = EXACTLY ONE holder — and ZERO is a breach.**
///
/// A linear resource must be *consumed*. That half of linearity is the half
/// every "linear types" README quietly drops, and it is the half that catches
/// the resource you provisioned, pay for, and forgot to use.
#[test]
fn t945_a_linear_resource_that_nothing_holds_is_a_breach_not_an_omission() {
    refutes(
        r#"resource Gpu { kind: http  endpoint: gpu.pool  lifetime: linear }"#,
        "axon-T945",
    );
}

// ── v2.87.0 — what a `lease` is, and what a `manifest` is not ─────────────
//
// These four pin a distinction that cost a false diagnostic in four PUBLISHED
// examples. `axon-T945` asks two questions and they need different answers:
// *"is anything USING this?"* and *"how many things POOL a connection to it?"*
//
// The whole class was invisible because `axon check` never ran this law: the
// CLI enters through `TypeChecker::check_with_warnings`, which had drifted into
// a separate copy of the pass list that no longer carried it. Every test here
// called `check` — the engine, not the path.

/// **A `lease` CONSUMES the resource**, so it satisfies linearity's
/// at-least-one half on its own.
///
/// `banking_reference.axon` shipped exactly this shape and the law called the
/// ledger unused.
#[test]
fn t945_a_lease_is_a_consumer_so_a_linear_resource_it_holds_is_used() {
    accepts(
        r#"
resource Ledger { kind: postgres  endpoint: ledger.main  lifetime: linear }
lease WriteWindow { resource: Ledger  duration: 30s  on_expire: anchor_breach }
"#,
    );
}

/// **…but a `lease` is NOT a pool holder**, so it does not compete with one.
///
/// This is v2.69.0's canonical governed shape — a resourced tool whose use is
/// bounded in time — and it is what the SSE lease-charging path is built on.
/// Counting the lease as a second holder would make the law refuse the pattern
/// the cycle beneath it exists to deliver. Measured: it did, until v2.87.0.
#[test]
fn t945_a_lease_over_an_affine_resource_a_tool_holds_is_not_sharing() {
    accepts(
        r#"
resource Api { kind: https  endpoint: vendor.base  lifetime: affine }
tool Search { provider: stub  resource: Api  runtime: search }
lease Gone { resource: Api  duration: 1h  on_expire: anchor_breach }
"#,
    );
}

/// **A `manifest` PROVISIONS; it does not use.** Listing a resource in a
/// deployment must not satisfy linearity, or the law means nothing — every
/// resource is listed somewhere.
#[test]
fn t945_a_manifest_listing_is_not_a_consumer() {
    refutes(
        r#"
resource Gpu { kind: http  endpoint: gpu.pool  lifetime: linear }
fabric Cloud { provider: aws  region: "us-east-1"  zones: 3 }
manifest Prod { resources: [Gpu]  fabric: Cloud  region: "us-east-1"  zones: 3 }
"#,
        "axon-T945",
    );
}

/// …and because a manifest is not a HOLDER either, the ordinary
/// declare-in-manifest / hold-in-store pattern stays legal for `linear`.
/// Counting it would have made this two holders.
#[test]
fn t945_manifest_plus_store_over_one_linear_resource_is_one_holder() {
    accepts(
        r#"
resource Db { kind: postgres  endpoint: db.main  lifetime: linear }
fabric Cloud { provider: aws  region: "us-east-1"  zones: 3 }
manifest Prod { resources: [Db]  fabric: Cloud  region: "us-east-1"  zones: 3 }
axonstore Records { backend: postgresql  resource: Db }
"#,
    );
}

#[test]
fn t945_a_linear_resource_with_two_holders_is_a_breach() {
    refutes(
        r#"
resource Db { kind: postgres  endpoint: db.main  lifetime: linear }
axonstore A { backend: postgresql  resource: Db }
axonstore B { backend: postgresql  resource: Db }
"#,
        "axon-T945",
    );
}

/// **`persistent` = the `!` exponential.** Sharing is what it is FOR — but you
/// have to *say so*. That is the entire point: a shared pool that nobody
/// declared shared is how connection exhaustion arrives without a suspect.
#[test]
fn t945_persistent_is_the_exponential_and_may_be_shared_freely() {
    accepts(
        r#"
resource Db { kind: postgres  endpoint: db.main  lifetime: persistent  capacity: 40 }
axonstore A { backend: postgresql  resource: Db }
axonstore B { backend: postgresql  resource: Db }
axonstore C { backend: postgresql  resource: Db }
"#,
    );
}

/// An `affine` resource with NO holder is fine — that is exactly what
/// distinguishes affine from linear. If this ever started failing, the two
/// lifetimes would have collapsed into one.
#[test]
fn t945_an_affine_resource_may_go_unused_that_is_what_makes_it_affine() {
    accepts(r#"resource Db { kind: postgres  endpoint: db.main  lifetime: affine }"#);
}

// ── axon-T946 — one source of truth ──────────────────────────────────────────

/// **The islands finding, written down deliberately.**
///
/// `resource.endpoint` and `axonstore.connection` are the same fact declared
/// twice, and nothing ever checked the two agreed. A store that declares BOTH is
/// that bug on purpose.
#[test]
fn t946_a_store_may_not_declare_the_same_fact_twice() {
    refutes(
        r#"
resource Db { kind: postgres  endpoint: db.main }
axonstore Users { backend: postgresql  resource: Db  connection: "postgres://elsewhere/app" }
"#,
        "axon-T946",
    );
}

#[test]
fn t946_a_store_naming_a_phantom_resource_is_refuted() {
    refutes(
        r#"axonstore Users { backend: postgresql  resource: NoSuchDb }"#,
        "axon-T946",
    );
}

/// **The soft migration, ratified.** `connection:` alone still compiles: it is
/// what the LIVE deployment runs on, and a hard cutover would break it. It
/// warns, and the store is ineligible for `lease`/`observe`/`reconcile` — you
/// cannot govern what you did not declare.
#[test]
fn t946_the_legacy_unresourced_store_still_compiles_the_migration_is_soft() {
    accepts(r#"axonstore Users { backend: postgresql  connection: "env:AXON_DB_URL" }"#);
}

// ── axon-T947 — manifest/fabric coherence ────────────────────────────────────

/// `within:` makes disjointness unrepresentable, but it opens ONE new way to
/// contradict yourself: a manifest naming fabric F while listing a resource that
/// lives `within:` G. Two declarations, one fact, disagreeing — which is the
/// exact disease v2.67.0 exists to cure, so it cannot be allowed in through the
/// back door.
#[test]
fn t947_a_manifest_cannot_relocate_a_resource_by_listing_it() {
    refutes(
        r#"
fabric   Prod  { provider: aws  region: "us-east-1" }
fabric   Stage { provider: aws  region: "us-east-1" }
resource Db    { kind: postgres  endpoint: db.main  within: Prod }
manifest Infra { fabric: Stage  resources: [Db] }
"#,
        "axon-T947",
    );
}

// ── the parser stops shrugging ───────────────────────────────────────────────

/// v2.67.0's dominant root cause was a parser that **silently skipped what it did
/// not understand** (`parse_block_step` → `skip_braced_block()`, which killed
/// four primitives outright). `parse_resource` had the same shrug: an unknown
/// field hit `_ => self.skip_value()`.
///
/// So `withn: Prod` would have been swallowed without a word, and the resource
/// would have governed nothing while looking governed. **A field the parser does
/// not know is a field the adopter believes in and the compiler does not.**
#[test]
fn a_misspelled_field_is_a_parse_error_not_a_shrug() {
    let src = r#"
fabric   Prod { provider: aws  region: "us-east-1" }
resource Db   { kind: postgres  endpoint: db.main  withn: Prod }
"#;
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let err = Parser::new(tokens)
        .parse()
        .expect_err("a misspelled field must NOT be silently skipped");
    let msg = err.message;
    assert!(
        msg.contains("withn"),
        "the diagnostic must name the field the adopter actually typed, got: {msg}"
    );
}
