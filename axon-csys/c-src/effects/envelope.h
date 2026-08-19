/*
 * v2.0.0 — Epistemic Envelope C23 Kernel (Theorem 5.1 in silicon).
 *
 * Public surface for the FlowEnvelope's epistemic enforcement primitives.
 * Called from the Rust shim `axon-csys::effects::envelope` which is
 * consumed by `axon-rs::wire_envelope::FlowEnvelope::seal`. Every
 * `transport: json` axonendpoint response on the v2.0.0 wire passes
 * through this kernel before serialization, making Theorem 5.1
 * structurally unbypassable from any Rust caller.
 *
 * ## Theorem 5.1 (paper section 5.1)
 *
 *   For any epistemic state E with `derived_status = true`, the
 *   certainty c is bounded c ≤ 0.99. No derived knowledge claims
 *   apodictic certainty; the language enforces evidentiary modesty
 *   in silicon.
 *
 * The 39.c.x algebra:
 *
 *   derived_status = true <=> (anchor_breaches > 0 || errors > 0
 *                              || steps_executed > 0 && inference_present)
 *
 * "inference_present" is a runtime determination by the producer (the
 * Rust converter) — flows that touched an LLM backend ARE derived;
 * pure retrieve flows over a deterministic store are NOT derived. The
 * C23 kernel does NOT inspect provenance internals; it trusts the
 * boolean `derived_status` flag set by the producer and applies the
 * structural ceiling. This split honours the pillar discipline: Rust
 * decides WHO is derived; C23 enforces WHAT the ceiling looks like.
 *
 * ## Layout discipline (per v1.19.0 pillar split)
 *
 *  - Public ABI is everything prefixed `axon_csys_envelope_*`. Order of
 *    functions in this file is the order in which Rust's `extern`
 *    block declares them (src/envelope.rs).
 *  - All public functions are pure + total + deterministic. No
 *    allocation. No syscalls. Constant time on bounded inputs.
 *  - The struct `axon_csys_envelope_t` is by-value through the FFI;
 *    no pointer lifetime concerns.
 *  - Attributes like `[[nodiscard]]` are intentionally omitted from
 *    this kernel's surface — the cross-platform CI matrix
 *    (MSVC /std:clatest + Apple clang pre-15 + GCC pre-13) doesn't
 *    have uniform support, and the kernel's discipline doesn't lose
 *    value without the compiler-side warning.
 */

#ifndef AXON_CSYS_EFFECTS_ENVELOPE_H
#define AXON_CSYS_EFFECTS_ENVELOPE_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Epistemic envelope FFI-stable struct. Byte-identical to the Rust
 * `EpistemicEnvelopeCRepr` in `axon-csys/src/envelope.rs`. Layout is
 * intentionally simple — three scalar fields, no pointers — so the
 * C23 <-> Rust ABI is trivially robust across calling conventions.
 *
 *  - certainty:      psi-vector E component (in [0.0, 1.0])
 *  - derived_status: producer's verdict on whether E is derived
 *  - epistemic_kind: closed-catalog ordinal tagging the dominant
 *                    posture of the envelope (0=Clean, 1=Derived,
 *                    2=Breached, 3=Degraded). Surfaces to telemetry
 *                    without re-deriving from the certainty value.
 */
typedef struct {
    double  certainty;
    bool    derived_status;
    uint8_t epistemic_kind;
} axon_csys_envelope_t;

/* Closed catalog of epistemic_kind ordinals — mirrored in Rust. */
#define AXON_CSYS_EPISTEMIC_CLEAN     0u
#define AXON_CSYS_EPISTEMIC_DERIVED   1u
#define AXON_CSYS_EPISTEMIC_BREACHED  2u
#define AXON_CSYS_EPISTEMIC_DEGRADED  3u

/*
 * Theorem 5.1 enforcement in silicon.
 *
 *   POST: result.certainty = clamp(env.certainty, [0.0, 0.99])
 *         IF env.derived_status, else result.certainty = env.certainty
 *   POST: env.certainty NaN / Inf normalised to 0.0 (defensive — a
 *         misbehaving Rust producer can't escape the bound via NaN)
 *   POST: derived_status + epistemic_kind passed through unchanged
 *
 * Pure function; deterministic; no allocation; constant time.
 *
 * Returns the input envelope with `certainty` clamped according to
 * Theorem 5.1.
 */
axon_csys_envelope_t axon_csys_envelope_validate_degradation(axon_csys_envelope_t env);

/*
 * Theorem 5.1 ceiling constant exported for cross-language drift
 * gates. The Rust shim compares this against its own const to detect
 * any divergence (which would imply a bug in either side).
 */
double axon_csys_envelope_theorem_5_1_ceiling(void);

/*
 * Apply the Theorem 5.1 ceiling unconditionally — used by the
 * defensive belt-and-suspenders path in the Rust seal() shim that
 * wants the absolute upper bound regardless of derived_status. NOT
 * the canonical path (canonical is `validate_degradation` which
 * honours `derived_status`); this is the secondary guard.
 *
 * POST: result = min(certainty, 0.99); NaN / Inf → 0.0
 */
double axon_csys_envelope_clamp_ceiling(double certainty);

#ifdef __cplusplus
}
#endif

#endif /* AXON_CSYS_EFFECTS_ENVELOPE_H */
