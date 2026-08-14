//! §Fase 113.d — **`lease`'s CT-2 Anchor Breach finally has a moment to fire in.**
//!
//! # The finding this closes, and why it is not like the others
//!
//! Every other §111 defect was *"the piece that makes them touch was missing"* —
//! a real engine with a dead wire. `lease` was worse, and the plan said so:
//!
//! > *"The README promises `lease` is a τ-decaying affine capability whose
//! > **post-expiry USE is a CT-2 Anchor Breach**. But a flow can never *use* a
//! > `resource` — so the breach has no moment to fire in. Wire the `LeaseKernel`
//! > perfectly and the headline guarantee is still **unreachable, not merely
//! > unwired**."*
//!
//! `LeaseKernel` was never broken. It has `acquire`, `use_token`, `release`, the
//! Anchor Breach, and all three `on_expire` policies, and its unit tests passed.
//! **It had no subject.** A guarantee about *using* a thing, in a language where
//! that thing could not be used, is not a weak guarantee — it is a **vacuous**
//! one. It cannot be violated, and therefore it cannot be kept.
//!
//! §113 created the subject. A flow uses an `axonstore`; the store runs on a
//! `resource`; **that store operation IS the use of the resource.** So:
//!
//! ```text
//!   resource  Db    { kind: postgres  endpoint: db.main  lifetime: affine }
//!   lease     Night { resource: Db  duration: 1h  on_expire: anchor_breach }
//!   axonstore Users { backend: postgresql  resource: Db }
//!
//!   …a `retrieve` from Users, an hour and a second later  →  CT-2 ANCHOR BREACH
//! ```
//!
//! These tests drive the kernel's clock forward and demand the breach.

use std::sync::{Arc, Mutex};

use axon::ir_nodes::{IRAxonStore, IRLease, IRResource};
use axon::resource_resolver::MapResourceResolver;
use axon::store::error::StoreError;
use axon::store::registry::StoreRegistry;

fn resource(name: &str, lifetime: &str) -> IRResource {
    IRResource {
        node_type: "resource",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        kind: "postgres".into(),
        endpoint: "db.main".into(),
        capacity: Some(20),
        lifetime: lifetime.into(),
        certainty_floor: None,
        shield_ref: String::new(),
        within: String::new(),
    }
}

fn lease(name: &str, resource_ref: &str, duration: &str, on_expire: &str) -> IRLease {
    IRLease {
        node_type: "lease",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        resource_ref: resource_ref.into(),
        duration: duration.into(),
        acquire: "on_start".into(),
        on_expire: on_expire.into(),
    }
}

/// A store on `resource_ref`. `backend: in_memory` keeps these tests free of a
/// database — the lease is charged in `resolve()`, **before** any pool is touched,
/// which is itself the point: an expired capability must not hand back a working
/// handle of any kind.
fn store(name: &str, resource_ref: &str) -> IRAxonStore {
    IRAxonStore {
        node_type: "axonstore",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        backend: "in_memory".into(),
        connection: String::new(),
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

// ── §Fase 122.b — the same breach, from a program on disk ──────────────────
//
// The gates below hand-build `IRResource`, `IRLease` and `IRAxonStore`, which
// proves the KERNEL. Law 4 asks for the PATH: the declarations as an adopter
// writes them, through the real compiler, into the same governed registry.

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
fn a_compiled_lease_permits_the_use_then_makes_it_a_breach() {
    const FIXTURE: &str = "tests/fixtures/fase122_b_resource/leased_store.axon";
    let ir = compile_fixture(FIXTURE);
    assert!(
        !ir.leases.is_empty(),
        "the fixture declares a `lease`; an empty catalog means the declaration \
         no longer reaches the IR"
    );

    let now = Arc::new(Mutex::new(chrono::Utc::now()));
    let c = now.clone();
    let reg = StoreRegistry::build_governed_with_clock(
        &ir.axonstore_specs,
        &ir.resources,
        &ir.leases,
        &resolver(),
        Box::new(move || *c.lock().unwrap()),
    )
    .expect("the lease declared in the fixture is acquired at build");

    // Within the DECLARED duration: the capability is held.
    reg.resolve("Users")
        .expect("a live lease must permit the use — the capability IS held");

    // Past it, the same use is the CT-2 Anchor Breach.
    *now.lock().unwrap() += chrono::Duration::seconds(3601);
    reg.resolve("Users").expect_err(
        "post-expiry USE is the CT-2 Anchor Breach the README promises — and before \
         §113 it could not even be expressed, because a flow could not USE a resource",
    );
}

// ── The breach ───────────────────────────────────────────────────────────────

/// **The assertion this entire fase exists to make possible.**
///
/// A live lease permits the use. Past τ, the same use is a **CT-2 Anchor Breach**.
///
/// Before §113 this test could not have been written at all — not because the
/// kernel was missing, but because there was no way to *use* a resource, so there
/// was no act for the breach to be about.
#[test]
fn using_a_store_after_its_lease_expires_is_a_ct2_anchor_breach() {
    // A clock we control: the lease decays because time passes, not because we
    // poked the kernel's internals.
    let now = Arc::new(Mutex::new(chrono::Utc::now()));
    let c = now.clone();

    let reg = StoreRegistry::build_governed_with_clock(
        &[store("Users", "Db")],
        &[resource("Db", "affine")],
        &[lease("Night", "Db", "1h", "anchor_breach")],
        &resolver(),
        Box::new(move || *c.lock().unwrap()),
    )
    .expect("the lease is acquired at build");

    // Within τ: the capability is held, the store resolves.
    reg.resolve("Users")
        .expect("a live lease must permit the use — the capability IS held");

    // …and an hour and a second later, the same use is a breach.
    *now.lock().unwrap() += chrono::Duration::seconds(3601);

    let err = reg
        .resolve("Users")
        .expect_err("post-expiry USE is the CT-2 Anchor Breach — the README's headline promise");

    match err {
        StoreError::LeaseExpired {
            ref store,
            ref resource,
            ref lease,
            ..
        } => {
            assert_eq!(store, "Users");
            assert_eq!(resource, "Db");
            assert_eq!(lease, "Night");
        }
        other => panic!("expected a CT-2 Anchor Breach, got: {other:?}"),
    }
    assert!(
        err.to_string().contains("ANCHOR BREACH"),
        "the diagnostic must name the breach, not merely fail"
    );
}

/// `on_expire: extend` — the window rolls forward and the use is permitted. The
/// capability is renewed, not abandoned.
///
/// This pins that the *policy* is honoured, not just the default: a lease kernel
/// that only ever breaches would be as dishonest as one that never does.
#[test]
fn on_expire_extend_renews_the_window_and_the_use_proceeds() {
    let now = Arc::new(Mutex::new(chrono::Utc::now()));
    let c = now.clone();

    let reg = StoreRegistry::build_governed_with_clock(
        &[store("Users", "Db")],
        &[resource("Db", "affine")],
        &[lease("Rolling", "Db", "1h", "extend")],
        &resolver(),
        Box::new(move || *c.lock().unwrap()),
    )
    .expect("acquired");

    *now.lock().unwrap() += chrono::Duration::seconds(3601);

    reg.resolve("Users")
        .expect("`on_expire: extend` renews the τ window — the use proceeds");

    // And the renewal is real: another hour on, it is live again rather than
    // presenting a revoked token.
    *now.lock().unwrap() += chrono::Duration::seconds(60);
    reg.resolve("Users").expect("the renewed token is the one now held");
}

/// `on_expire: release` — the capability is surrendered cleanly at τ, and the use
/// **still cannot proceed**. Releasing is not permission; it is the opposite.
#[test]
fn on_expire_release_surrenders_the_capability_and_the_use_is_refused() {
    let now = Arc::new(Mutex::new(chrono::Utc::now()));
    let c = now.clone();

    let reg = StoreRegistry::build_governed_with_clock(
        &[store("Users", "Db")],
        &[resource("Db", "affine")],
        &[lease("Batch", "Db", "1h", "release")],
        &resolver(),
        Box::new(move || *c.lock().unwrap()),
    )
    .expect("acquired");

    *now.lock().unwrap() += chrono::Duration::seconds(3601);

    let err = reg.resolve("Users").expect_err(
        "a released lease is a capability NO LONGER HELD — the store op must refuse. \
         Letting it through would make `release` a synonym for `ignore`.",
    );
    assert!(matches!(err, StoreError::LeaseExpired { .. }));
}

// ── The laws around it ───────────────────────────────────────────────────────

/// A `persistent` resource is the `!` exponential — unbounded, with **no τ to
/// decay**. A lease over it is meaningless, and the kernel has always said so.
/// Now something actually asks it.
#[test]
fn a_lease_over_a_persistent_resource_is_refused_there_is_no_tau_to_decay() {
    let err = StoreRegistry::build_governed(
        &[store("Users", "Db")],
        &[resource("Db", "persistent")],
        &[lease("Night", "Db", "1h", "anchor_breach")],
        &resolver(),
    )
    .expect_err("a lease over `persistent` has nothing to expire");
    assert!(err.to_string().contains("persistent"), "got: {err}");
}

/// **The un-resourced store is INELIGIBLE for lease governance** — §113's
/// ratified posture, enforced.
///
/// *You cannot govern what you did not declare.* And silently governing it would
/// be worse than not governing it: the adopter would be relying on a guarantee
/// they never asked for and cannot see in their source.
#[test]
fn a_legacy_unresourced_store_is_not_governed_by_any_lease() {
    let now = Arc::new(Mutex::new(chrono::Utc::now()));
    let c = now.clone();

    let mut legacy = store("Legacy", "");
    legacy.connection = "postgres://legacy/app".into();

    let reg = StoreRegistry::build_governed_with_clock(
        &[legacy],
        &[resource("Db", "affine")],
        &[lease("Night", "Db", "1h", "anchor_breach")],
        &resolver(),
        Box::new(move || *c.lock().unwrap()),
    )
    .expect("built");

    // Long past τ — and the un-resourced store is untouched by it.
    *now.lock().unwrap() += chrono::Duration::seconds(7200);
    reg.resolve("Legacy")
        .expect("a store that names no resource is governed by no lease over one");
}

/// A program with no leases pays nothing and behaves exactly as before.
#[test]
fn a_program_with_no_leases_is_unchanged() {
    let reg = StoreRegistry::build_with_resources(
        &[store("Users", "Db")],
        &[resource("Db", "affine")],
        &resolver(),
    )
    .expect("built");
    reg.resolve("Users").expect("no lease ⇒ nothing to charge");
}
