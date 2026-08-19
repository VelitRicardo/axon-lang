//! AXON Runtime — Cognitive Immune System kernels (v1.1.0).
//!
//! Direct port of `axon/runtime/immune/`. Formal spec:
//! [`docs/paper_immune_v2.md`](../../../../docs/paper_immune_v2.md).

pub mod detector;
pub mod heal;
pub mod health_report;
pub mod reflex;

pub use detector::{AnomalyDetector, KLDistribution};
pub use heal::{
    ApplyFn, Clock, HealDecision, HealKernel, HealOutcome, Patch, PatchState, ShieldApproveFn,
    SynthesizeFn, default_apply, default_shield_approve, default_synthesize,
};
pub use health_report::{
    HealthReport, VALID_LEVELS, certainty_from_kl, level_at_least, level_from_kl, level_order,
    make_health_report,
};
pub use reflex::{ReflexEngine, ReflexOutcome};
