//! v2.40.0 — drift gate for the native `cache` primitive doc.
//!
//! The canonical programs published in `knowledge/primitives/cache.md` must
//! round-trip through the same `axon-frontend` pipeline the `axon` CLI uses —
//! the "published grammar MUST compile" discipline.
//!
//! Mirrors `technician_canonical_programs.rs`.

use axon_emcp::compiler_pipeline::{run, Outcome};

fn must_compile(label: &str, source: &str) {
    match run(source, label) {
        Outcome::Ok { .. } => {}
        Outcome::Err {
            stage,
            errors,
            warnings,
        } => panic!(
            "{label}: expected well-formed program, got {stage:?} failure:\n\
             errors   = {errors:#?}\n\
             warnings = {warnings:#?}\n\
             source   = {source}"
        ),
    }
}

/// The published `cache.md` surface example: a pure default cache + a widened,
/// ttl-bounded, invalidation-wired cache referenced by a non-pure tool.
#[test]
fn cache_doc_example_compiles() {
    let src = r#"
type WeatherEvent { city: String }
channel WeatherUpdated { message: WeatherEvent }

cache DefaultPure { default: true }

cache WeatherCache {
    backend: redis
    ttl: 5m
    apply_to_effects: [pure, network]
    invalidate_on: [WeatherUpdated]
}

tool Fingerprint { provider: http  effects: <pure>  parameters: { input: String } }
tool Weather {
    provider: http
    effects: <network>
    parameters: { city: String }
    cache: WeatherCache
}
"#;
    must_compile("cache/canonical", src);
}

/// A `retrieve.cache:` over a ttl-bounded cache compiles (a retrieve is never
/// pure, so the referenced cache carries a finite ttl).
#[test]
fn retrieve_cache_compiles() {
    let src = r#"
type Row { id: String }
axonstore Tenants { backend: postgresql connection: "env:DB" }
channel TenantsChanged { message: Row }
cache TenantsCache { ttl: 30s apply_to_effects: [storage] invalidate_on: [TenantsChanged] }
flow ListTenants() -> Unit {
    retrieve Tenants { where: "active = true" cache: TenantsCache as: rows }
}
"#;
    must_compile("cache/retrieve", src);
}
