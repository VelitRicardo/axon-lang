/*
 * v2.0.0 — Epistemic Envelope C23 Kernel implementation.
 *
 * Theorem 5.1 enforcement in silicon. The kernel is intentionally
 * tiny — three pure functions, no allocation, no syscalls. The whole
 * point is that the bound is STRUCTURALLY ENFORCED at the C/Rust
 * boundary so a misbehaving Rust producer cannot escape it via any
 * normal code path.
 *
 * See `envelope.h` for the full contract + Theorem 5.1 statement.
 */

#include "envelope.h"
#include <math.h>

/* Theorem 5.1 — the ceiling constant. */
#define AXON_CSYS_THEOREM_5_1_CEILING 0.99

/*
 * Helper: defensive normalisation. NaN / +Inf / -Inf / negative inputs
 * are coerced to 0.0 so a misbehaving Rust producer cannot inject
 * arithmetic poison via the FFI boundary. Finite positive values are
 * passed through.
 */
static double axon_csys_envelope_normalise(double c) {
    if (!isfinite(c)) {
        return 0.0;
    }
    if (c < 0.0) {
        return 0.0;
    }
    if (c > 1.0) {
        return 1.0;
    }
    return c;
}

axon_csys_envelope_t axon_csys_envelope_validate_degradation(axon_csys_envelope_t env) {
    /*
     * Step 1 — defensive normalisation. Any NaN / Inf / out-of-range
     * input is coerced into [0.0, 1.0] BEFORE the ceiling check. This
     * guarantees the post-condition `result.certainty ∈ [0.0, 1.0]`
     * regardless of caller misbehaviour.
     */
    env.certainty = axon_csys_envelope_normalise(env.certainty);

    /*
     * Step 2 — Theorem 5.1 enforcement. Derived states get the
     * ceiling clamp; non-derived (apodictic-by-construction) states
     * pass through.
     */
    if (env.derived_status && env.certainty > AXON_CSYS_THEOREM_5_1_CEILING) {
        env.certainty = AXON_CSYS_THEOREM_5_1_CEILING;
    }

    /* derived_status + epistemic_kind pass through unchanged. */
    return env;
}

double axon_csys_envelope_theorem_5_1_ceiling(void) {
    return AXON_CSYS_THEOREM_5_1_CEILING;
}

double axon_csys_envelope_clamp_ceiling(double certainty) {
    certainty = axon_csys_envelope_normalise(certainty);
    if (certainty > AXON_CSYS_THEOREM_5_1_CEILING) {
        certainty = AXON_CSYS_THEOREM_5_1_CEILING;
    }
    return certainty;
}
