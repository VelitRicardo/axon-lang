//! v2.41.0 — the mathematical core of Directed Creative Synthesis (`forge`).
//!
//! This module is the rigor of the primitive: the pure, deterministic functions
//! that turn "creativity" from a hopeful prompt into a **measured, enforced
//! quantity**. The four-phase orchestration (the LLM calls) lives in
//! `flow_dispatcher::cognitive::run_forge`; this module is what it measures and
//! decides with, and it is fully unit-testable without a model.
//!
//! The honest mathematics (see the cycle doc section 4):
//! - **Boden's creativity taxonomy** (Boden 1990) → a closed catalog of
//! sampling parameters. A designed operationalization, not a law.
//! - **Novelty via NCD**: Kolmogorov complexity K(x) is UNCOMPUTABLE,
//!   so novelty cannot be computed exactly. We use the **Normalized Compression
//!   Distance** — the standard *computable* approximation of the Normalized
//!   Information Distance, a universal metric grounded in Kolmogorov complexity
//!   (Li, Chen, Li, Ma, Vitányi, "The similarity metric", IEEE TIT 2004). We
//!   name the approximation explicitly rather than pretend to compute K.
//! - **Fail-closed verification**: a forge returns a value ONLY if it
//!   provably clears a novelty floor AND its coherence floor. A derivative
//!   output is NEVER passed off as creative.

use flate2::write::DeflateEncoder;
use flate2::Compression;
use std::io::Write;

// ── Boden creativity taxonomy → sampling parameters ──────────────────

/// The sampling profile a creativity `mode` maps to. `tau_base` is the base
/// temperature; `freedom` and `rule_flexibility` are carried for prompt framing
/// and future backends. Values match the published README taxonomy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodenProfile {
    pub tau_base: f64,
    pub freedom: f64,
    pub rule_flexibility: f64,
}

/// Map a `forge.mode:` to its Boden profile. An unknown/empty mode defaults to
/// `exploratory` (the type-checker rejects a non-empty unknown mode via T868;
/// this default covers the omitted-mode case, the design decision).
pub fn boden_profile(mode: &str) -> BodenProfile {
    match mode {
        "combinatorial" => BodenProfile {
            tau_base: 0.9,
            freedom: 0.8,
            rule_flexibility: 0.3,
        },
        "transformational" => BodenProfile {
            tau_base: 1.2,
            freedom: 1.0,
            rule_flexibility: 0.9,
        },
        // "exploratory" + default.
        _ => BodenProfile {
            tau_base: 0.7,
            freedom: 0.6,
            rule_flexibility: 0.5,
        },
    }
}

/// Incubation temperature: τ_eff = τ_base × (0.5 + 0.5·novelty). The novelty
/// operator blends divergence into the effective temperature (the design decision; matches
/// the README — transformational at novelty 0.85 ⇒ 1.2 × 0.925 = 1.11).
pub fn incubation_temperature(mode: &str, novelty: f64) -> f64 {
    boden_profile(mode).tau_base * (0.5 + 0.5 * novelty.clamp(0.0, 1.0))
}

// ── NCD: the computable Kolmogorov-novelty proxy ─────────────────────

/// Compressed length C(x) using deflate — the |·| in the NCD formula. Deflate
/// is a legitimate NCD compressor (CompLearn/zlib precedent).
pub fn compressed_len(data: &[u8]) -> usize {
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::best());
    if enc.write_all(data).is_err() {
        return data.len();
    }
    enc.finish().map(|v| v.len()).unwrap_or(data.len())
}

/// Normalized Compression Distance:
/// `NCD(x,y) = [C(xy) − min(C(x),C(y))] / max(C(x),C(y))` ∈ ~`[0,1]`.
/// `0` ⇒ x,y are near-identical (one compresses away given the other, i.e.
/// derivative); `→1` ⇒ independent (genuinely novel).
pub fn ncd(x: &[u8], y: &[u8]) -> f64 {
    let cx = compressed_len(x) as f64;
    let cy = compressed_len(y) as f64;
    let mut xy = Vec::with_capacity(x.len() + y.len());
    xy.extend_from_slice(x);
    xy.extend_from_slice(y);
    let cxy = compressed_len(&xy) as f64;
    let denom = cx.max(cy);
    if denom == 0.0 {
        return 0.0;
    }
    ((cxy - cx.min(cy)) / denom).clamp(0.0, 1.0)
}

/// Novelty of an output O against the obvious baseline B (the Preparation
/// phase's conventional reading of the seed): ν(O) = NCD(B, O) — "how much of O
/// is NOT already implied by the obvious reading of the seed".
pub fn novelty_score(baseline: &str, output: &str) -> f64 {
    ncd(baseline.as_bytes(), output.as_bytes())
}

// ── Novelty floor from the `novelty:` parameter ──────────────────────

/// The calibrated NCD band. On coherent, related text NCD occupies a bounded
/// range (a value near 1 means the output is unrelated/gibberish), so the
/// required floor scales the `novelty:` knob across `[MIN, MAX]` — high novelty
/// demands more divergence-from-the-obvious while staying coherent. Named
/// calibration, tunable, NOT a derived law.
pub const NOVELTY_FLOOR_MIN: f64 = 0.15;
pub const NOVELTY_FLOOR_MAX: f64 = 0.60;

pub fn novelty_floor(novelty: f64) -> f64 {
    let n = novelty.clamp(0.0, 1.0);
    NOVELTY_FLOOR_MIN + (NOVELTY_FLOOR_MAX - NOVELTY_FLOOR_MIN) * n
}

// ── Illumination selection: argmax over feasible branches ────────────

/// One illumination branch (a crystallized candidate) with its measured scores.
#[derive(Debug, Clone, PartialEq)]
pub struct Branch {
    pub output: String,
    /// Coherence in `[0,1]` — the anchor's confidence for this branch (or a
    /// self-consistency score when the forge declares no `constraints:` anchor).
    pub coherence: f64,
    /// Measured novelty ν = NCD(baseline, output).
    pub novelty: f64,
}

pub const W_COHERENCE: f64 = 0.5;
pub const W_NOVELTY: f64 = 0.5;

/// Utility of a branch: `U = w_c·coherence + w_n·min(ν/ν_floor, 1)`. The novelty
/// term saturates at the floor (beyond "novel enough", extra divergence does
/// not keep buying utility — it would trade away coherence).
pub fn branch_utility(b: &Branch, nov_floor: f64) -> f64 {
    let nov_term = if nov_floor > 0.0 {
        (b.novelty / nov_floor).min(1.0)
    } else {
        1.0
    };
    W_COHERENCE * b.coherence + W_NOVELTY * nov_term
}

/// Best-of-N: select the index of the argmax-utility branch among the FEASIBLE
/// set (coherence ≥ floor). `None` if no branch is feasible ⇒ the forge fails
/// (the design decision, no derivative is smuggled through).
pub fn select_illumination(
    branches: &[Branch],
    coherence_floor: f64,
    nov_floor: f64,
) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, b) in branches.iter().enumerate() {
        if b.coherence < coherence_floor {
            continue;
        }
        let u = branch_utility(b, nov_floor);
        if best.map(|(_, bu)| u > bu).unwrap_or(true) {
            best = Some((i, u));
        }
    }
    best.map(|(i, _)| i)
}

// ── Fail-closed verification ─────────────────────────────────────────

/// The verdict of the Verification phase. `Accepted` carries the MEASURED
/// novelty + coherence so the runtime can bind the typed value and the audit
/// can record exactly how creative the result was.
#[derive(Debug, Clone, PartialEq)]
pub enum ForgeVerdict {
    Accepted {
        output: String,
        novelty: f64,
        coherence: f64,
    },
    Rejected(ForgeRejection),
}

/// Why a forge failed closed — a structured reason, never a silent empty result.
#[derive(Debug, Clone, PartialEq)]
pub enum ForgeRejection {
    /// No illumination branch cleared the anchor's coherence floor.
    NoFeasibleBranch,
    /// The best feasible branch is too derivative of the obvious baseline.
    NoveltyFloorBreached { measured: f64, floor: f64 },
}

impl ForgeRejection {
    /// A stable slug for the structured `FlowEnvelope.error` / audit row.
    pub fn slug(&self) -> &'static str {
        match self {
            ForgeRejection::NoFeasibleBranch => "forge.no_feasible_branch",
            ForgeRejection::NoveltyFloorBreached { .. } => "forge.novelty_floor_breached",
        }
    }
}

/// The final gate. `winner` is the coherence-feasible selection (or `None` if
/// the feasible set was empty). Require ν ≥ novelty_floor — else reject. This is
/// the load-bearing guarantee: a forge returns a value ONLY if it provably
/// cleared BOTH floors.
pub fn verify(winner: Option<&Branch>, nov_floor: f64) -> ForgeVerdict {
    match winner {
        None => ForgeVerdict::Rejected(ForgeRejection::NoFeasibleBranch),
        Some(b) => {
            if b.novelty < nov_floor {
                ForgeVerdict::Rejected(ForgeRejection::NoveltyFloorBreached {
                    measured: b.novelty,
                    floor: nov_floor,
                })
            } else {
                ForgeVerdict::Accepted {
                    output: b.output.clone(),
                    novelty: b.novelty,
                    coherence: b.coherence,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boden_matches_published_taxonomy() {
        assert_eq!(boden_profile("combinatorial").tau_base, 0.9);
        assert_eq!(boden_profile("exploratory").tau_base, 0.7);
        assert_eq!(boden_profile("transformational").tau_base, 1.2);
        assert_eq!(boden_profile("transformational").rule_flexibility, 0.9);
        // Unknown/empty → exploratory default.
        assert_eq!(boden_profile("").tau_base, 0.7);
    }

    #[test]
    fn incubation_temperature_matches_readme_worked_example() {
        // transformational (τ_base 1.2) at novelty 0.85 ⇒ 1.2 × 0.925 = 1.11.
        let t = incubation_temperature("transformational", 0.85);
        assert!((t - 1.11).abs() < 1e-9, "got {t}");
    }

    #[test]
    fn ncd_of_identical_is_near_zero() {
        let s = "the quick brown fox jumps over the lazy dog, again and again";
        assert!(ncd(s.as_bytes(), s.as_bytes()) < 0.15, "identical should be ~0");
    }

    #[test]
    fn ncd_derivative_below_unrelated() {
        let baseline = "a serene mountain lake at dawn with mist over calm water and pine trees";
        // A near-restatement (derivative) of the baseline.
        let derivative = "a calm mountain lake at dawn, mist over the still water and pine trees";
        // A genuinely different concept.
        let novel = "recursive fractal cathedrals grown from bioluminescent coral under an ocean of liquid mercury";
        let d = novelty_score(baseline, derivative);
        let n = novelty_score(baseline, novel);
        assert!(d < n, "derivative ({d}) must score lower novelty than a divergent concept ({n})");
    }

    #[test]
    fn novelty_floor_is_monotonic_within_band() {
        assert_eq!(novelty_floor(0.0), NOVELTY_FLOOR_MIN);
        assert_eq!(novelty_floor(1.0), NOVELTY_FLOOR_MAX);
        assert!(novelty_floor(0.3) < novelty_floor(0.8));
    }

    #[test]
    fn selection_picks_highest_utility_feasible_branch() {
        let branches = vec![
            Branch { output: "A".into(), coherence: 0.9, novelty: 0.05 }, // coherent but derivative
            Branch { output: "B".into(), coherence: 0.8, novelty: 0.50 }, // balanced — should win
            Branch { output: "C".into(), coherence: 0.4, novelty: 0.90 }, // novel but below floor
        ];
        let floor = 0.5; // coherence floor
        let nov_floor = novelty_floor(0.6);
        let idx = select_illumination(&branches, floor, nov_floor).unwrap();
        assert_eq!(idx, 1, "the balanced feasible branch wins");
    }

    #[test]
    fn no_feasible_branch_rejects() {
        let branches = vec![
            Branch { output: "A".into(), coherence: 0.3, novelty: 0.9 },
            Branch { output: "B".into(), coherence: 0.4, novelty: 0.9 },
        ];
        assert!(select_illumination(&branches, 0.7, 0.4).is_none());
        assert_eq!(verify(None, 0.4), ForgeVerdict::Rejected(ForgeRejection::NoFeasibleBranch));
    }

    #[test]
    fn verify_rejects_a_derivative_winner_fail_closed() {
        // A coherent but derivative winner (novelty below floor) MUST be
        // rejected — never passed off as creative.
        let winner = Branch { output: "derivative".into(), coherence: 0.95, novelty: 0.10 };
        let v = verify(Some(&winner), novelty_floor(0.8));
        match v {
            ForgeVerdict::Rejected(ForgeRejection::NoveltyFloorBreached { measured, floor }) => {
                assert!(measured < floor);
            }
            other => panic!("expected novelty-floor rejection, got {other:?}"),
        }
    }

    #[test]
    fn verify_accepts_a_novel_coherent_winner() {
        let winner = Branch { output: "novel".into(), coherence: 0.85, novelty: 0.55 };
        let v = verify(Some(&winner), novelty_floor(0.6));
        assert!(matches!(v, ForgeVerdict::Accepted { .. }));
    }
}
