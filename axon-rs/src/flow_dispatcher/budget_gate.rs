//! v2.69.0 — **the budget gate, in ONE place, on EVERY tool path.**
//!
//! # The live bug this closes
//!
//! `budget { rate: 100 per minute on Tool(Search) }` parses, type-checks
//! (`axon-T830`–`T834`), and its kernel is **real**: a refilling token bucket, a
//! tumbling window, fail-closed, with `on_exhausted ∈ {block, defer, shed}`
//! honoured.
//!
//! Until v2.69.0 it was wired to **exactly one** dispatch site —
//! `pure_shape::run_step_streaming_tool` — which you reach only if **all three**
//! hold:
//!
//! 1. the tool declares `effects: stream:*` (`derive_is_streaming`), **and**
//! 2. the call happens inside a `daemon`, **and**
//! 3. you are on the enterprise supervisor (OSS's `run_daemon` has zero callers).
//!
//! **The canonical `use Tool(…)` path had zero budget references.** `ctx.budget`
//! appeared in one file in the entire crate.
//!
//! And the sharpest part of it:
//!
//! > `advertised.rs` certifies `tool` as **Real**, citing as its proof
//! > *`lambda_tools::dispatch_use_tool_real`* — **the very function where its
//! > budget did not run.**
//!
//! So an adopter wrote a budget over their vendor, the compiler accepted it, and
//! **in an ordinary flow it did nothing.** That is not a design gap; it is a live
//! governance hole on the primitive the product's safety story rests on.
//!
//! # Why this is a module and not a copy-paste
//!
//! The obvious fix is to paste the gate into `lambda_tools` too. That would make
//! **two copies of one law** — and v2.67.0 established what that costs: the key-shape
//! check for `axon-T850` had been inlined at three sites, and *three copies of a
//! law is how the islands happened*. The copies drift, and the one nobody looks at
//! quietly stops meaning anything.
//!
//! There is now **one** charge point. Every tool path calls it.
//!
//! # Why the gate must precede the emission
//!
//! A budget charged *after* the call has already been made bounds nothing — the
//! vendor was already hit, the money was already spent. Over-emission has to be
//! **impossible by construction**, not merely reported. So [`charge`] runs before
//! any request is issued, and a denial means **the call is never made.**

use crate::flow_dispatcher::{DispatchCtx, DispatchError};
use crate::runtime::budget_kernel::GateDecision;

/// What the budget decided about one tool emission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetGrant {
    /// The token was consumed (or no budget governs this tool). **Proceed.**
    ///
    /// An unbudgeted tool is granted unconditionally — byte-identical to
    /// pre-v2.28.0. A budget you did not declare cannot deny you.
    Granted,
    /// `on_exhausted: shed` — best-effort. **The call is NOT made**, but the flow
    /// continues: the step completes with empty output.
    ///
    /// The caller MUST record the shed in the audit trail. A skipped call that
    /// leaves no trace is indistinguishable from a call that returned nothing,
    /// and those are very different facts.
    Shed { retry_at_ms: i64 },
}

/// Charge one tool emission against the program's `budget`.
///
/// Called **before** the request is issued, on every tool path. `Err` ⇒ the call
/// must not happen.
///
/// - `block` (the default) ⇒ [`DispatchError::EffectQuotaExhausted`] — fail-closed.
/// - `defer`               ⇒ [`DispatchError::EffectDeferred`] — a *distinct* error,
///                           so a supervisor reschedules instead of failing.
/// - `shed`                ⇒ [`BudgetGrant::Shed`] — skip the call, continue the flow.
pub fn charge(ctx: &DispatchCtx, tool_name: &str) -> Result<BudgetGrant, DispatchError> {
    let Some(budget) = &ctx.budget else {
        // No budget declared for this program. Unbudgeted tools are granted
        // unconditionally — the v2.28.0 doctrine: a budget you did not write cannot
        // deny you.
        return Ok(BudgetGrant::Granted);
    };

    let now = chrono::Utc::now();
    let decision = budget
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .gate(tool_name, now);

    let GateDecision::Deny {
        retry_at,
        on_exhausted,
    } = decision
    else {
        // Granted — the token(s) were consumed; the emission proceeds.
        return Ok(BudgetGrant::Granted);
    };

    let retry_at_ms = retry_at.timestamp_millis();
    match on_exhausted.as_str() {
        "shed" => Ok(BudgetGrant::Shed { retry_at_ms }),
        "defer" => Err(DispatchError::EffectDeferred {
            effect: tool_name.to_string(),
            retry_at_ms,
        }),
        // `block` and anything else → fail-closed. An unknown policy must never
        // widen the grant: the whole point of a quota is that exceeding it is
        // impossible, and a policy the runtime does not understand is not a
        // licence to proceed.
        _ => Err(DispatchError::EffectQuotaExhausted {
            effect: tool_name.to_string(),
            retry_at_ms,
        }),
    }
}
