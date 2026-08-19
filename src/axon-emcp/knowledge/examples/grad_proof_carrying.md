---
name: grad_proof_carrying
title: "`grad` — the proof-carrying derivative (v2.65.0)"
summary: "Symbolic differentiation at COMPILE TIME over the closed `Expr` (v2.26.0): `grad <let> wrt [vars] as <name>` differentiates the expression a prior rich `let` bound, simplifies deterministically, and stores the result IN THE IR — inspectable, re-proven at deploy (PCC `GradientSoundness` re-derives and refutes a hand-edited gradient), evaluated at runtime by the same total evaluator `let` uses. No tape, no finite differences, no narration. A non-differentiable construct is a compile refusal naming construct + position (axon-T931 — never a silent zero); the target must be a PRIOR rich `let` (axon-T932). Change `y * y` to `y % 2` and this program stops compiling."
topic: composition
primitives:
  - flow
---

// The proof-carrying derivative (v2.65.0). The gradient this flow returns
// was DERIVED at compile time — symbolically, over the closed Expr —
// and only EVALUATED at runtime. No tape, no finite differences.

flow Score(x: Float, y: Float) -> Text {
    // The differentiable material: a RICH `let` (its expression AST
    // rides the IR — that is what `grad` differentiates).
    let total = 3.0 * x + y * y

    // ∂total/∂x = 3.0 (folded by the deterministic simplifier);
    // ∂total/∂y = y + y (product rule, the 1· stripped).
    // Both land in the IR as inspectable artifacts, and the PCC class
    // `GradientSoundness` re-derives them at deploy — a hand-edited
    // gradient cannot ship. At runtime: g = {"x": 3.0, "y": 2y}.
    //
    // Swap `y * y` for `y % 2` above and compilation REFUSES
    // (axon-T931, naming `mod` and its position): a gradient over a
    // non-differentiable construct does not exist, and axon does not
    // fabricate one — no silent zeros, no step-size lies.
    grad total wrt [x, y] as g

    return g
}
