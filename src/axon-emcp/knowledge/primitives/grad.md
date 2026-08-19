---
name: grad
summary: "The proof-carrying derivative — SYMBOLIC differentiation over the v2.26.0 expression language at compile time; the gradient IS IR (PCC GradientSoundness) and the runtime only evaluates."
category: operators
top_level: false
since: v2.65.0
grammar: |
  let g = grad(<expr>, <wrt-binding>)
---

# `grad`

`grad` is the **proof-carrying derivative**: symbolic differentiation
over the v2.26.0 expression language, at COMPILE time.

## What the runtime actually does (v2.65.0)

The gradient **is IR**: differentiation happens in the frontend, the
derivative expression ships inside the artifact under the PCC
`GradientSoundness` witness, and the runtime only **evaluates** it —
zero tokens, no model in the loop, no numeric approximation.

## Proof

`axon-rs/tests/grad_runtime.rs` +
`axon-frontend/tests/grad_grammar.rs`.

## See also

- `axon://primitives/compute` — the sibling: named pure functions over
  the same expression language.
