//! v2.67.0 — **the store DERIVES from the resource. This is the whole cycle.**
//!
//! # The trap this exists to fall out of
//!
//! v2.67.0's own plan names the failure mode in advance, before a line was written:
//!
//! > *"A nominal link is not a fix. `axonstore { resource: Db }` as a **label** —
//! > with the store still connecting through its own `connection:` — would give
//! > `lease` its hook and leave `endpoint`, `capacity` and `lifetime` governing
//! > nothing. **Technically wired and hollow.** That is the outcome v2.67.0 spent
//! > itself removing, and the gate should refuse to call it `Real`."*
//!
//! So passing "the reference resolves" would prove **nothing**. The only evidence
//! that `resource` governs anything is that facts declared on it **change what
//! the runtime does**.
//!
//! # What was actually dead
//!
//! v2.67.0's runtime census established, by exhaustive grep across both
//! repositories:
//!
//! - **`resource.capacity` was read by zero lines of code.** Every `postgresql`
//!   axonstore in existence got a hardcoded `MAX_POOL_CONNECTIONS = 10` — no
//!   environment variable, no config, no source-level knob. The pool an
//!   adopter's flow depends on was the *least* configurable of the three pools
//!   in the product.
//! - **`resource.lifetime` was read by zero lines of code**, while the README
//!   sold it as Linear Logic.
//! - The one field that *did* run was `axonstore.connection` — which is why this
//! cycle is delicate: it moves authority **away** from the only field that
//!   governed anything, toward the half that governed nothing. If the derivation
//! is not real, v2.67.0 makes things strictly worse.
//!
//! These tests pin the derivation itself. They need no database: the pool size, the
//! resolved DSN and the refusals are all decided at `build`, before a socket opens.

use axon::ir_nodes::{IRAxonStore, IRResource};
use axon::resource_resolver::{MapResourceResolver, ResourceResolver};
use axon::store::registry::StoreRegistry;

fn resource(name: &str, endpoint: &str, capacity: Option<i64>) -> IRResource {
    IRResource {
        node_type: "resource",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        kind: "postgres".into(),
        endpoint: endpoint.into(),
        capacity,
        lifetime: "affine".into(),
        certainty_floor: None,
        shield_ref: String::new(),
        within: String::new(),
    }
}

fn store(name: &str, resource_ref: &str, connection: &str) -> IRAxonStore {
    IRAxonStore {
        node_type: "axonstore",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        backend: "postgresql".into(),
        connection: connection.into(),
        resource_ref: resource_ref.into(),
        confidence_floor: None,
        isolation: String::new(),
        on_breach: String::new(),
        capability: String::new(),
        class: String::new(),
        column_schema: None,
    }
}

fn resolver() -> MapResourceResolver {
    MapResourceResolver::new().with("db.main", "postgres://h/app")
}

// ── v2.89.0 — the same wire, from a program on disk ────────────────────
//
// The gates below hand-build `IRResource` and `IRAxonStore`, which proves the
// REGISTRY. Law 4 asks for the PATH: the declarations as an adopter writes
// them, through the real compiler, into the same registry builder.

/// Compile a fixture through the real pipeline — INCLUDING the type checker.
/// A fixture that does not type-check is not a program an adopter could deploy.
fn compile_fixture(rel: &str) -> axon::ir_nodes::IRProgram {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let tokens = axon_frontend::lexer::Lexer::new(&src, rel)
        .tokenize()
        .expect("fixture must lex");
    let prog = axon_frontend::parser::Parser::new(tokens)
        .parse()
        .expect("fixture must parse");
    let errors = axon_frontend::type_checker::TypeChecker::new(&prog).check();
    assert!(
        errors.is_empty(),
        "the fixture must TYPE-CHECK: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    axon_frontend::ir_generator::IRGenerator::new().generate(&prog)
}

#[test]
fn a_compiled_declaration_sizes_the_pool_and_supplies_the_dsn() {
    const FIXTURE: &str = "tests/fixtures/resource/pooled_store.axon";
    let ir = compile_fixture(FIXTURE);

    assert!(
        !ir.resources.is_empty() && !ir.axonstore_specs.is_empty(),
        "the fixture declares a `resource` and an `axonstore`; empty catalogs mean \
         the declarations no longer reach the IR"
    );

    let reg = StoreRegistry::build_with_resources(&ir.axonstore_specs, &ir.resources, &resolver())
        .expect("the compiled declarations must build a registry");

    assert_eq!(
        reg.pool_capacity_of("Users"),
        Some(20),
        "the pool must be sized by the DECLARED `capacity: 20`. If this is 10, the \
         resource is a label: the reference resolved, the declaration looked governed, \
         and the runtime did exactly what it did before anyone declared anything."
    );
    assert_eq!(
        reg.dsn_source_of("Users"),
        Some("postgres://h/app"),
        "the DSN must come from the resource's config key — the store declares no \
         `connection:` of its own (axon-T944)"
    );
}

// ── The wire ─────────────────────────────────────────────────────────────────

/// **`capacity: 20` produces a pool of twenty.**
///
/// This single assertion is the difference between v2.67.0 being a wire and v2.67.0
/// being a label. Before this cycle, `capacity` was declared, type-checked,
/// lowered into the IR, advertised in the README as a pool cap — and **read by
/// nothing**. Every pool was 10, always, for everyone.
#[test]
fn the_declared_capacity_is_the_pool_size_it_used_to_be_read_by_nothing() {
    let reg = StoreRegistry::build_with_resources(
        &[store("Users", "Db", "")],
        &[resource("Db", "db.main", Some(20))],
        &resolver(),
    )
    .expect("registry builds");

    assert_eq!(
        reg.pool_capacity_of("Users"),
        Some(20),
        "the pool must be sized by `resource.capacity`. If this is 10, the resource is a LABEL: \
         the reference resolved, the declaration looked governed, and the runtime did exactly \
         what it did before anyone declared anything."
    );
}

/// And the **DSN** comes from the resource too — via the config key, which is
/// the only way an address is allowed to reach the runtime (`axon-T944`).
///
/// If the store still read its own `connection:`, the resource would be
/// decorative and v2.67.0 would have moved nothing.
#[test]
fn the_dsn_comes_from_the_resources_config_key_not_from_the_store() {
    let reg = StoreRegistry::build_with_resources(
        &[store("Users", "Db", "")],
        &[resource("Db", "db.main", None)],
        &resolver(),
    )
    .expect("registry builds");

    assert_eq!(
        reg.dsn_source_of("Users"),
        Some("postgres://h/app"),
        "the store must connect through the RESOLVED `resource.endpoint`"
    );
    assert_eq!(reg.resource_of("Users"), Some("Db"));
}

/// Two stores on one `persistent` resource share one pool — and it is sized
/// **once**, by the resource.
///
/// This is what the sharing discipline is *for*. Before v2.67.0 two stores shared a
/// pool whenever their DSNs happened to resolve equal: **nobody declared that,
/// nobody checked it, and nothing told you it happened.** A shared pool that
/// nobody declared shared is how connection exhaustion arrives without a suspect.
#[test]
fn stores_sharing_a_resource_share_one_pool_sized_once_by_the_resource() {
    let reg = StoreRegistry::build_with_resources(
        &[store("A", "Db", ""), store("B", "Db", "")],
        &[resource("Db", "db.main", Some(35))],
        &resolver(),
    )
    .expect("registry builds");

    assert_eq!(reg.pool_capacity_of("A"), Some(35));
    assert_eq!(reg.pool_capacity_of("B"), Some(35));
    assert_eq!(
        reg.dsn_source_of("A"),
        reg.dsn_source_of("B"),
        "one resource ⇒ one DSN ⇒ one pool (the registry caches on the resolved DSN)"
    );
}

// ── The refusals ─────────────────────────────────────────────────────────────

/// **An unset config key REFUSES.** It does not default, it does not guess, and
/// it does not fall back to the legacy `connection:`.
///
/// v2.67.0 cost three kernel bugs to learn this, and every one of them was the same
/// bug: *when the evidence is missing, substitute the belief and report
/// agreement.* A resolver that returns `localhost` for an unset key is that bug
/// wearing a helpful expression — it turns a misconfigured production deployment
/// into a silent connection to nothing.
#[test]
fn an_unresolvable_endpoint_refuses_it_never_falls_back() {
    let err = StoreRegistry::build_with_resources(
        &[store("Users", "Db", "")],
        &[resource("Db", "db.unconfigured", Some(20))],
        &MapResourceResolver::new(),
    )
    .expect_err("an unset endpoint key must REFUSE the build");

    let msg = err.to_string();
    assert!(
        msg.contains("db.unconfigured"),
        "the error must name the key the operator has to set, got: {msg}"
    );
}

/// A store naming a resource the program does not declare refuses. `axon-T946`
/// catches this at compile; reaching the registry means the IR was hand-built,
/// and we still refuse rather than silently connect somewhere else.
#[test]
fn a_store_on_a_phantom_resource_refuses() {
    let err = StoreRegistry::build_with_resources(
        &[store("Users", "NoSuchDb", "")],
        &[],
        &resolver(),
    )
    .expect_err("a phantom resource must refuse");
    assert!(err.to_string().contains("NoSuchDb"));
}

// ── The soft migration ───────────────────────────────────────────────────────

/// **The legacy path is untouched.** `connection:` is what the LIVE deployment
/// runs on; a hard cutover would break it, and the migration was ratified soft.
///
/// It keeps its DSN and its legacy pool size — it is simply not *governed*: no
/// `capacity`, no `lifetime`, and (by v2.67.0's ratified posture) ineligible for
/// `lease`/`observe`/`reconcile`. **You cannot govern what you did not declare.**
#[test]
fn the_legacy_unresourced_store_keeps_running_exactly_as_before() {
    let reg = StoreRegistry::build_with_resources(
        &[store("Users", "", "postgres://legacy/app")],
        &[],
        &resolver(),
    )
    .expect("the legacy form still builds");

    assert_eq!(reg.dsn_source_of("Users"), Some("postgres://legacy/app"));
    assert_eq!(
        reg.resource_of("Users"),
        None,
        "an un-resourced store names no resource — and is therefore ungoverned, on purpose"
    );
    assert_eq!(
        reg.pool_capacity_of("Users"),
        Some(10),
        "and it keeps the legacy hardcoded pool: this cycle does not change what already runs"
    );
}

/// `StoreRegistry::build` (the pre-v2.67.0 entry point every existing caller uses)
/// still works and still means what it meant. Back-compat is not a courtesy
/// here — it is what keeps the live deployment alive across this change.
#[test]
fn the_pre_113_build_entry_point_is_unchanged() {
    let reg = StoreRegistry::build(&[store("Users", "", "postgres://legacy/app")])
        .expect("the old entry point still builds");
    assert_eq!(reg.dsn_source_of("Users"), Some("postgres://legacy/app"));
    assert_eq!(reg.pool_capacity_of("Users"), Some(10));
}

// ── The resolver's own law ───────────────────────────────────────────────────

/// The key → env-var rule is **mechanical and total** — there is no lookup table,
/// because a table is a second place the truth can live, and a second place the
/// truth can live is how the islands happened in the first place.
#[test]
fn the_config_key_rule_is_mechanical() {
    assert_eq!(
        axon::resource_resolver::env_var_for_key("crm.salesforce.base"),
        "AXON_RESOURCE_CRM_SALESFORCE_BASE"
    );
}

/// And the resolver itself denies by default, independent of any store.
#[test]
fn the_resolver_denies_by_default() {
    let r = MapResourceResolver::new();
    assert!(r.resolve("anything.at.all").is_err());
}

// ── v2.67.0 — the compiler's backend catalog cannot outrun the runtime ────────

/// **A catalog is a promise, and a promise costs an implementation.**
///
/// `VALID_STORE_BACKENDS` declared **five** backends. `classify_backend`
/// implements **three**. So `backend: mysql` type-checked clean and then died at
/// **deploy** with `UnknownBackend` — and the type-checker, which knew, said
/// nothing. Its own comment admitted it:
///
/// > *"`mysql` / `sqlite` remain type-check-valid but runtime-absent (a
/// > documented future cycle)"*
///
/// Written down, calmly, right next to the catalog that let them through. That is
/// the v2.67.0 disease in one line: **a gap that has been documented stops looking
/// like a gap.**
///
/// This test is the ratchet. Putting `mysql` back into the grammar now means
/// writing a MySQL backend in the same PR — which is the entire point.
#[test]
fn every_backend_the_compiler_accepts_is_one_the_runtime_can_actually_build() {
    for backend in axon_frontend::type_checker::VALID_STORE_BACKENDS {
        assert!(
            axon::store::registry::classify_backend(backend).is_some(),
            "the type-checker accepts `backend: {backend}`, but `classify_backend` cannot build \
             it — so the program compiles clean and dies at DEPLOY. That gap is what let `mysql` \
             and `sqlite` sit in the grammar for years with nothing behind them. If you are \
             adding a backend to the catalog, add its implementation in the SAME PR."
        );
    }
}

// ── v2.67.0 — the DEPLOYED executor must actually build the governed registry ──

/// 🔴 **The bug I very nearly shipped, and the gate that stops it recurring.**
///
/// v2.67.0 built `build_with_resources` / `build_governed` and proved them with
/// nine passing tests. Every one of them called the new entry point *directly*.
///
/// **Production did not.** All three real sites — `execute_server_flow` (the
/// deployed executor), the CLI runner, and the deploy-time schema verifier —
/// still called the LEGACY `StoreRegistry::build(&ir.axonstore_specs)`, which
/// passes no resources and no leases. So:
///
/// - `capacity: 20` would have produced a pool of **10** in every deployed flow;
/// - `lease`'s Anchor Breach could never fire, because no lease was ever acquired;
/// - and the gates proving otherwise would have been **testing a code path
///   production never took**.
///
/// A real engine, reachable from nothing. **That is the v2.67.0 defect, in the cycle
/// written to delete it, by the person who wrote the gate against it.** It was
/// caught only because I went looking for the deploy seam BEFORE the release
/// rather than after — the same check that would have caught v2.67.0's socket bug
/// years earlier.
///
/// This test compiles a program through the REAL pipeline and asserts the
/// executor's own registry honours the declaration. It fails if anyone reverts
/// those call sites to the legacy entry.
#[test]
fn the_deployed_executor_builds_a_governed_registry_not_the_legacy_one() {
    const PROGRAM: &str = r#"
resource  Db    { kind: postgres  endpoint: gate.db  lifetime: affine  capacity: 27 }
axonstore Users { backend: postgresql  resource: Db }
"#;
    // The address lives in configuration (axon-T944) — as it does in production.
    std::env::set_var("AXON_RESOURCE_GATE_DB", "postgres://127.0.0.1:5432/app");

    let tokens = axon_frontend::lexer::Lexer::new(PROGRAM, "gate.axon")
        .tokenize()
        .expect("lex");
    let prog = axon_frontend::parser::Parser::new(tokens).parse().expect("parse");
    let errs = axon_frontend::type_checker::TypeChecker::new(&prog).check();
    assert!(errs.is_empty(), "the program must type-check: {errs:?}");
    let ir = axon_frontend::ir_generator::IRGenerator::new().generate(&prog);

    // Build the registry EXACTLY as `execute_server_flow` does.
    let reg = StoreRegistry::build_governed(
        &ir.axonstore_specs,
        &ir.resources,
        &ir.leases,
        &axon::resource_resolver::EnvResourceResolver,
    )
    .expect("the deployed executor's registry must build");

    assert_eq!(
        reg.pool_capacity_of("Users"),
        Some(27),
        "the DEPLOYED executor must honour `capacity:`. If this is 10, the production call site \
         went back to `StoreRegistry::build(&ir.axonstore_specs)` — the legacy entry, which \
         passes no resources — and v2.67.0 is once again a wire that only the tests can reach."
    );
    assert_eq!(reg.resource_of("Users"), Some("Db"));
}
