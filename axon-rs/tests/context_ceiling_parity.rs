//! v2.22.0 — cross-crate parity: the frontend's `MAX_KNOWN_CONTEXT_WINDOW`
//! (the `axon-T809` ceiling, used at `axon check`) MUST equal the runtime model
//! catalog's largest context window (`axon::backends::model_catalog`). The two
//! live in different crates (the frontend can't depend on axon-rs), so the value
//! is mirrored — and this test pins the mirror: adding a larger-context model to
//! the v2.22.0 catalog without bumping the frontend ceiling fails CI. The v2.21.0
//! two-representation discipline ([[feedback-published-grammar-must-compile]]).

#[test]
fn frontend_t809_ceiling_equals_the_runtime_catalog_max() {
    let frontend = axon_frontend::type_checker::MAX_KNOWN_CONTEXT_WINDOW;
    let runtime = axon::backends::model_catalog::max_canonical_context_window();
    assert_eq!(
        frontend, runtime,
        "axon-T809 ceiling drift: the frontend MAX_KNOWN_CONTEXT_WINDOW ({frontend}) \
         must equal the v2.22.0 catalog's largest window ({runtime}). Bump the frontend \
         const when adding a larger-context model to the catalog."
    );
}
