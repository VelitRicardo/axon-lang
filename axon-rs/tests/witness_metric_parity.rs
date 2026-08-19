//! v2.23.0 — cross-crate parity: the frontend's closed witness-metric catalog
//! (`axon check`, `axon-E0790`) MUST equal the runtime catalog
//! (`axon::advantage_witness`). The two live in different crates (the frontend
//! can't depend on axon-rs), so the catalog is mirrored — and this test pins the
//! mirror: adding a metric to one without the other fails CI. The v2.21.0 / v2.22.0
//! two-representation discipline ([[feedback-published-grammar-must-compile]]).

#[test]
fn witness_metric_catalog_is_identical_across_crates() {
    assert_eq!(
        axon_frontend::type_checker::WITNESS_METRICS,
        axon::advantage_witness::WITNESS_METRICS,
        "witness-metric catalog drift: `axon check` and the runtime must agree on \
         which advantage metrics exist. Update BOTH WITNESS_METRICS constants."
    );
}

#[test]
fn every_catalog_slug_round_trips_through_the_runtime_enum() {
    // Each frontend-accepted metric must map to a runtime `AdvantageMetric` —
    // otherwise `axon check` blesses a metric the evaluator can't compute.
    for slug in axon_frontend::type_checker::WITNESS_METRICS {
        assert!(
            axon::advantage_witness::AdvantageMetric::from_slug(slug).is_some(),
            "metric '{slug}' is in the frontend catalog but the runtime can't parse it"
        );
    }
}
